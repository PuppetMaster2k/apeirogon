// SPDX-License-Identifier: GPL-3.0-or-later

//! X11 edge glow and landing mark; the contract is in [`super`].
//!
//! Both are override-redirect windows (no decoration, no focus, no window
//! manager involvement) with an empty SHAPE input region, so clicks pass
//! through. They use the root visual and a solid background, so no compositor
//! is needed. The EWMH hints are set anyway for compositors that look at them.
//!
//! X11 has no reliable reduce-motion setting, so the landing mark is a short
//! static marker that never animates; that satisfies reduce-motion everywhere.

mod geometry;

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use a7n_proto::v1::Edge;
use x11rb::connection::Connection;
use x11rb::protocol::shape::{ConnectionExt as _, SK, SO};
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ClipOrdering, ConfigureWindowAux, ConnectionExt as _,
    CreateWindowAux, PropMode, Rectangle, StackMode, Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use super::look::{self, BEAM_THICKNESS, Rect};
use super::{Color, Command, Feedback, Landing, Look, Style, channel};
use crate::input::Bounds;
use geometry::{glow_rect, pick_monitor};

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        window_type: b"_NET_WM_WINDOW_TYPE".as_slice(),
        type_notification: b"_NET_WM_WINDOW_TYPE_NOTIFICATION".as_slice(),
        state: b"_NET_WM_STATE".as_slice(),
        state_above: b"_NET_WM_STATE_ABOVE".as_slice(),
        skip_taskbar: b"_NET_WM_STATE_SKIP_TASKBAR".as_slice(),
        skip_pager: b"_NET_WM_STATE_SKIP_PAGER".as_slice(),
    }
}

/// Accent colour (RGB) of the glow and the landing mark.
const ACCENT_RGB: u32 = rgb_pixel(Color::Accent.rgb(true));

const fn rgb_pixel([r, g, b]: [u8; 3]) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}
/// Side of the landing mark in pixels.
const LANDING_SIZE: u32 = 24;
/// How long the landing mark stays.
const LANDING_DURATION: Duration = Duration::from_millis(600);

pub(super) fn start() -> Option<Feedback> {
    let overlay = Overlay::new()?;
    let (feedback, mut receiver) = channel();
    let (forward, commands) = mpsc::channel();
    // The tokio receiver cannot time out, so this thread only forwards; the
    // X thread waits on the std channel with the landing deadline.
    thread::Builder::new()
        .name("a7n-feedback-recv".into())
        .spawn(move || {
            while let Some(command) = receiver.blocking_recv() {
                if forward.send(command).is_err() {
                    break;
                }
            }
        })
        .ok()?;
    thread::Builder::new()
        .name("a7n-feedback-x11".into())
        .spawn(move || overlay.run(&commands))
        .ok()?;
    Some(feedback)
}

struct Overlay {
    connection: RustConnection,
    root: Window,
    glow: Window,
    landing: Window,
}

impl Overlay {
    fn new() -> Option<Self> {
        let (connection, screen) = x11rb::connect(None).ok()?;
        let root = connection.setup().roots.get(screen)?.root;
        connection.shape_query_version().ok()?.reply().ok()?;
        let atoms = Atoms::new(&connection).ok()?.reply().ok()?;
        let glow = create_overlay_window(&connection, screen, &atoms)?;
        let landing = create_overlay_window(&connection, screen, &atoms)?;
        connection.flush().ok()?;
        Some(Self {
            connection,
            root,
            glow,
            landing,
        })
    }

    /// Handles commands until the daemon drops its handle or X fails.
    fn run(&self, commands: &mpsc::Receiver<Command>) {
        let mut landing_deadline: Option<Instant> = None;
        loop {
            let received = match landing_deadline {
                Some(deadline) => {
                    commands.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                }
                None => commands
                    .recv()
                    .map_err(|_| mpsc::RecvTimeoutError::Disconnected),
            };
            let handled = match received {
                Ok(command) => {
                    if matches!(command, Command::Landing { .. }) {
                        landing_deadline = Some(Instant::now() + LANDING_DURATION);
                    }
                    self.handle(command)
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    landing_deadline = None;
                    self.unmap(self.landing)
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            if handled.is_none() {
                break;
            }
        }
    }

    fn handle(&self, command: Command) -> Option<()> {
        match command {
            Command::Edge {
                display,
                edge,
                progress,
                look,
            } => self.show_glow(display, edge, progress, look),
            Command::Clear => self.unmap(self.glow),
            Command::Landing { x, y, look } => self.show_landing(x, y, look),
            Command::Tick => Some(()),
        }
    }

    fn show_landing(&self, x: i32, y: i32, look: Look) -> Option<()> {
        let (radius, _) = look.landing.frame(Duration::ZERO, LANDING_DURATION, false);
        let diameter = f64::from(LANDING_SIZE) * look.size.multiplier() * radius;
        let mark = Rect {
            x: f64::from(x) - diameter / 2.0,
            y: f64::from(y) - diameter / 2.0,
            width: diameter,
            height: diameter,
        }
        .bounds();
        let diameter = u16::try_from(mark.width).ok()?;
        let shape = landing_shape(
            diameter,
            look.landing == Landing::Dot,
            look.size.multiplier(),
        );
        self.connection
            .shape_rectangles(
                SO::SET,
                SK::BOUNDING,
                ClipOrdering::UNSORTED,
                self.landing,
                0,
                0,
                &shape,
            )
            .ok()?;
        self.connection
            .change_window_attributes(
                self.landing,
                &ChangeWindowAttributesAux::new().background_pixel(rgb_pixel(look.color.rgb(true))),
            )
            .ok()?;
        self.connection
            .clear_area(false, self.landing, 0, 0, 0, 0)
            .ok()?;
        self.show(self.landing, mark)
    }

    fn show_glow(&self, display: Bounds, edge: Edge, progress: u16, look: Look) -> Option<()> {
        let pointer = self
            .connection
            .query_pointer(self.root)
            .ok()?
            .reply()
            .ok()?;
        let monitors = crate::input::x11::monitors(&self.connection, self.root)
            .unwrap_or_else(|| vec![display]);
        let (x, y) = (i32::from(pointer.root_x), i32::from(pointer.root_y));
        let monitor = pick_monitor(&monitors, display, x, y);
        let along = if matches!(edge, Edge::Top | Edge::Bottom) {
            x
        } else {
            y
        };
        let color = rgb_pixel(look.color.rgb(true));
        self.connection
            .change_window_attributes(
                self.glow,
                &ChangeWindowAttributesAux::new().background_pixel(color),
            )
            .ok()?;
        self.connection
            .clear_area(false, self.glow, 0, 0, 0, 0)
            .ok()?;
        let rect = if look.style == Style::Beam {
            look::beam_rect(
                Rect::from_bounds(monitor),
                edge,
                (f64::from(x), f64::from(y)),
                progress,
                BEAM_THICKNESS * look.size.multiplier(),
            )
            .map(Rect::bounds)
        } else {
            glow_rect(monitor, edge, along, progress, look.size)
        };
        match rect {
            Some(rect) => self.show(self.glow, rect),
            None => self.unmap(self.glow),
        }
    }

    /// Places the window and raises it above the others.
    fn show(&self, window: Window, rect: Bounds) -> Option<()> {
        let aux = ConfigureWindowAux::new()
            .x(rect.x)
            .y(rect.y)
            .width(rect.width)
            .height(rect.height)
            .stack_mode(StackMode::ABOVE);
        self.connection.configure_window(window, &aux).ok()?;
        self.connection.map_window(window).ok()?;
        self.connection.flush().ok()
    }

    fn unmap(&self, window: Window) -> Option<()> {
        self.connection.unmap_window(window).ok()?;
        self.connection.flush().ok()
    }
}

/// Rasterize a circle into scanlines for SHAPE, without requiring a compositor.
fn landing_shape(diameter: u16, dot: bool, scale: f64) -> Vec<Rectangle> {
    const RING_WIDTH: f64 = 2.0;
    let radius = f64::from(diameter) / 2.0;
    let inner = if dot {
        0.0
    } else {
        (radius - RING_WIDTH * scale).max(0.0)
    };
    let mut shape = Vec::new();
    for y in 0..diameter {
        let dy = f64::from(y) + 0.5 - radius;
        let mut start = None;
        for x in 0..=diameter {
            let dx = f64::from(x) + 0.5 - radius;
            let distance = dx * dx + dy * dy;
            let filled = x < diameter && distance <= radius * radius && distance >= inner * inner;
            if filled {
                start.get_or_insert(x);
            } else if let Some(from) = start.take() {
                shape.push(Rectangle {
                    x: i16::try_from(from).unwrap_or(0),
                    y: i16::try_from(y).unwrap_or(0),
                    width: x - from,
                    height: 1,
                });
            }
        }
    }
    shape
}

/// An unmapped, click-through, override-redirect window in the accent colour.
fn create_overlay_window(
    connection: &RustConnection,
    screen_number: usize,
    atoms: &Atoms,
) -> Option<Window> {
    let screen = connection.setup().roots.get(screen_number)?;
    let window = connection.generate_id().ok()?;
    let aux = CreateWindowAux::new()
        .override_redirect(1)
        .background_pixel(ACCENT_RGB);
    connection
        .create_window(
            screen.root_depth,
            window,
            screen.root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            screen.root_visual,
            &aux,
        )
        .ok()?;
    // An empty input region lets every click fall through to the windows below.
    connection
        .shape_rectangles(
            SO::SET,
            SK::INPUT,
            ClipOrdering::UNSORTED,
            window,
            0,
            0,
            &[],
        )
        .ok()?;
    connection
        .change_property32(
            PropMode::REPLACE,
            window,
            atoms.window_type,
            AtomEnum::ATOM,
            &[atoms.type_notification],
        )
        .ok()?;
    connection
        .change_property32(
            PropMode::REPLACE,
            window,
            atoms.state,
            AtomEnum::ATOM,
            &[atoms.state_above, atoms.skip_taskbar, atoms.skip_pager],
        )
        .ok()?;
    Some(window)
}
