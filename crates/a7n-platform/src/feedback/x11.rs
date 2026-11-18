// SPDX-License-Identifier: GPL-3.0-or-later

//! X11 edge glow and landing mark; the contract is in [`super`].
//!
//! Override-redirect windows have empty input regions. Composited desktops
//! use an ARGB visual; others retain the solid background and SHAPE fallback.
//! Animation follows the desktop preference; unknown desktops stay static.

mod blit;
mod geometry;
mod window;

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use a7n_proto::v1::Edge;
use x11rb::connection::Connection;
use x11rb::protocol::shape::{ConnectionExt as _, SK, SO};
use x11rb::protocol::xproto::{
    ChangeWindowAttributesAux, ClipOrdering, ConfigureWindowAux, ConnectionExt as _, StackMode,
    Window,
};
use x11rb::rust_connection::RustConnection;

use super::look::{self, Rect};
use super::raster::{self, Along, Arrival, EdgeEffect, Frame};
use super::timeline::{End, Timeline, frame_interval};
use super::{Command, Feedback, Landing, Look, channel};
use crate::input::Bounds;
use blit::Argb;
use geometry::{glow_rect, pick_monitor};
use window::{create_overlay_window, landing_shape};

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
    let (forward, commands) = mpsc::sync_channel(super::COMMAND_CAPACITY);
    // The tokio receiver cannot time out, so this thread only forwards; the
    // X thread waits on the std channel with the landing deadline.
    thread::Builder::new()
        .name("a7n-feedback-recv".into())
        .spawn(move || {
            while let Some(command) = receiver.blocking_recv() {
                // A stalled X thread drops commands rather than queueing them
                // without bound, like the sender side of the channel does.
                if let Err(mpsc::TrySendError::Disconnected(_)) = forward.try_send(command) {
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
    argb: Option<Argb>,
    edge_frame: Frame,
    timeline: Timeline,
    edge_started: Instant,
    animate: bool,
    landing_frame: Frame,
    frame_interval: Duration,
}

impl Overlay {
    fn new() -> Option<Self> {
        let (connection, screen) = x11rb::connect(None).ok()?;
        let root = connection.setup().roots.get(screen)?.root;
        connection.shape_query_version().ok()?.reply().ok()?;
        let atoms = Atoms::new(&connection).ok()?.reply().ok()?;
        let argb = Argb::new(&connection, screen);
        let glow = create_overlay_window(&connection, screen, &atoms, argb.as_ref())?;
        let landing = create_overlay_window(&connection, screen, &atoms, argb.as_ref())?;
        connection.flush().ok()?;
        Some(Self {
            connection,
            root,
            glow,
            landing,
            argb,
            edge_frame: Frame::default(),
            timeline: Timeline::default(),
            edge_started: Instant::now(),
            animate: animations_enabled(),
            landing_frame: Frame::default(),
            frame_interval: frame_interval(None),
        })
    }

    /// Handles commands until the daemon drops its handle or X fails.
    fn run(mut self, commands: &mpsc::Receiver<Command>) {
        loop {
            let edge_playing = self.animating_edge();
            let playing = edge_playing || self.timeline.landing_visible();
            let received = if playing {
                commands.recv_timeout(self.frame_interval)
            } else {
                commands
                    .recv()
                    .map_err(|_| mpsc::RecvTimeoutError::Disconnected)
            };
            match received {
                Ok(command) => {
                    if self.handle(command).is_none() {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if (edge_playing || self.animating_edge()) && self.animate_edge().is_none() {
                break;
            }
            if self.timeline.landing_visible() && self.animate_landing().is_none() {
                break;
            }
        }
    }

    /// Reads the refresh rate once per effect, not on every pointer update.
    fn pace_edge(&mut self, monitor: Bounds) {
        if !self.animating_edge() {
            self.frame_interval = self.display_frame_interval(monitor);
        }
    }

    fn display_frame_interval(&self, monitor: Bounds) -> Duration {
        frame_interval(crate::input::x11::refresh_hz(
            &self.connection,
            self.root,
            monitor,
        ))
    }

    fn animating_edge(&self) -> bool {
        self.argb.is_some() && self.timeline.edge_playing(self.animate)
    }

    fn animate_edge(&mut self) -> Option<()> {
        if !self
            .timeline
            .render_edge(&mut self.edge_frame, self.edge_started.elapsed())
        {
            return self.unmap(self.glow);
        }
        self.argb
            .as_mut()?
            .blit(&self.connection, self.glow, &self.edge_frame)
    }

    fn animate_landing(&mut self) -> Option<()> {
        if !self
            .timeline
            .render_landing(&mut self.landing_frame, self.edge_started.elapsed())
        {
            return self.unmap(self.landing);
        }
        if let Some(argb) = &mut self.argb {
            argb.blit(&self.connection, self.landing, &self.landing_frame)?;
        }
        Some(())
    }

    fn handle(&mut self, command: Command) -> Option<()> {
        match command {
            Command::Edge {
                display,
                edge,
                progress,
                look,
            } => self.show_glow(display, edge, progress, look),
            Command::Clear | Command::Crossed => {
                self.timeline
                    .end(End::for_command(command), self.edge_started.elapsed());
                if self.argb.is_none() {
                    self.unmap(self.glow)?;
                }
                Some(())
            }
            Command::Landing { x, y, look } => self.show_landing(x, y, look),
            Command::LandingAtCursor { look } => {
                let pointer = self
                    .connection
                    .query_pointer(self.root)
                    .ok()?
                    .reply()
                    .ok()?;
                self.show_landing(i32::from(pointer.root_x), i32::from(pointer.root_y), look)
            }
            Command::Tick => Some(()),
        }
    }

    fn show_landing(&mut self, x: i32, y: i32, look: Look) -> Option<()> {
        if self.argb.is_none() && !look.show_marker {
            return Some(());
        }
        self.timeline.land(look, self.edge_started.elapsed());
        let diameter = if self.argb.is_some() {
            look.arrival_diameter()
        } else {
            f64::from(look.size.pixels(LANDING_SIZE))
        };
        let monitor = self.landing_monitor(x, y)?;
        self.frame_interval = self.display_frame_interval(monitor);
        let area = Rect::from_bounds(monitor);
        let point = (f64::from(x), f64::from(y));
        let geometry = look::landing_rect(area, point, diameter);
        let rect = geometry.bounds();
        self.landing_frame.arrival = Some(Arrival::at(geometry, point, 1.0));
        self.landing_frame.animate = self.animate && self.argb.is_some();
        if self.argb.is_some() {
            return self.show_composited_landing(rect);
        }
        self.show_shaped_landing(x, y, rect, look)
    }

    fn landing_monitor(&self, x: i32, y: i32) -> Option<Bounds> {
        let monitors = crate::input::x11::monitors(&self.connection, self.root).unwrap_or_default();
        let screen = self
            .connection
            .setup()
            .roots
            .iter()
            .find(|screen| screen.root == self.root)?;
        let fallback = Bounds::from((
            0,
            0,
            u32::from(screen.width_in_pixels),
            u32::from(screen.height_in_pixels),
        ));
        Some(pick_monitor(&monitors, fallback, x, y))
    }

    fn show_composited_landing(&mut self, rect: Bounds) -> Option<()> {
        self.landing_frame
            .prepare(rect.width, rect.height, 1.0, self.animate);
        self.timeline
            .render_landing(&mut self.landing_frame, self.edge_started.elapsed());
        self.show(self.landing, rect)?;
        self.argb
            .as_mut()?
            .blit(&self.connection, self.landing, &self.landing_frame)
    }

    fn show_shaped_landing(&mut self, x: i32, y: i32, rect: Bounds, look: Look) -> Option<()> {
        self.landing_frame.resize(rect.width, rect.height);
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
        self.paint_solid(self.landing, look)?;
        self.show(self.landing, mark)
    }

    fn show_glow(&mut self, display: Bounds, edge: Edge, progress: u16, look: Look) -> Option<()> {
        let (monitor, pointer) = self.glow_monitor(display)?;
        self.pace_edge(monitor);
        if self.argb.is_none() {
            self.paint_solid(self.glow, look)?;
        }
        let rect = if self.argb.is_some() {
            let depth = f64::from(raster::OVERLAY_DEPTH);
            look::edge_strip(Rect::from_bounds(monitor), edge, depth).map(Rect::bounds)
        } else {
            let along = if matches!(edge, Edge::Top | Edge::Bottom) {
                pointer.0
            } else {
                pointer.1
            };
            glow_rect(monitor, edge, along, progress, look.size)
        };
        let Some(rect) = rect else {
            self.timeline
                .end(End::Abandoned, self.edge_started.elapsed());
            return self.unmap(self.glow);
        };
        self.show(self.glow, rect)?;
        if self.argb.is_some() {
            self.push_edge_effect(rect, monitor, pointer, edge, progress, look)?;
        }
        Some(())
    }

    fn glow_monitor(&self, display: Bounds) -> Option<(Bounds, (i32, i32))> {
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
        Some((monitor, (x, y)))
    }

    fn paint_solid(&self, window: Window, look: Look) -> Option<()> {
        let color = rgb_pixel(look.color.sample(0.0, true));
        self.connection
            .change_window_attributes(
                window,
                &ChangeWindowAttributesAux::new().background_pixel(color),
            )
            .ok()?;
        self.connection.clear_area(false, window, 0, 0, 0, 0).ok()?;
        Some(())
    }

    fn push_edge_effect(
        &mut self,
        rect: Bounds,
        monitor: Bounds,
        pointer: (i32, i32),
        edge: Edge,
        progress: u16,
        look: Look,
    ) -> Option<()> {
        let argb = self.argb.as_mut()?;
        self.edge_frame
            .prepare(rect.width, rect.height, 1.0, self.animate);
        let along = Along::across(
            Rect::from_bounds(monitor),
            Rect::from_bounds(rect),
            (f64::from(pointer.0), f64::from(pointer.1)),
            edge,
            1.0,
        );
        let effect = EdgeEffect {
            look,
            edge,
            progress,
            along,
        };
        self.timeline.push(effect, self.edge_started.elapsed());
        self.timeline
            .render_edge(&mut self.edge_frame, self.edge_started.elapsed());
        argb.blit(&self.connection, self.glow, &self.edge_frame)
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
        if self.argb.is_none() {
            self.connection.flush().ok()?;
        }
        Some(())
    }

    fn unmap(&self, window: Window) -> Option<()> {
        self.connection.unmap_window(window).ok()?;
        self.connection.flush().ok()
    }
}

/// Unknown desktops conservatively use static frames, as the old backend did.
fn animations_enabled() -> bool {
    use std::process::Command;

    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if desktop.contains("gnome") {
        return Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "enable-animations"])
            .output()
            .is_ok_and(|output| output.status.success() && output.stdout.trim_ascii() == b"true");
    }
    if desktop.contains("kde") {
        for executable in ["kreadconfig6", "kreadconfig5"] {
            if let Ok(output) = Command::new(executable)
                .args([
                    "--file",
                    "kdeglobals",
                    "--group",
                    "KDE",
                    "--key",
                    "AnimationDurationFactor",
                    "--default",
                    "1",
                ])
                .output()
                && output.status.success()
            {
                return std::str::from_utf8(&output.stdout)
                    .ok()
                    .and_then(|text| text.trim().parse::<f64>().ok())
                    .is_some_and(|factor| factor > 0.0);
            }
        }
    }
    false
}
