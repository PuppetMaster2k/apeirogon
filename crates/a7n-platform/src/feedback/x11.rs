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
            animate: blit::animations_enabled(),
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
        self.argb.is_some()
            && self
                .timeline
                .edge_playing(self.animate, self.edge_started.elapsed())
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
                self.timeline.end(
                    if command == Command::Crossed {
                        End::Crossed
                    } else {
                        End::Abandoned
                    },
                    self.edge_started.elapsed(),
                );
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

    fn show_landing(&mut self, x: i32, y: i32, mut look: Look) -> Option<()> {
        if self.argb.is_none() {
            if !look.show_marker {
                return Some(());
            }
            look.show_edge = false;
        }
        self.timeline.land(look, self.edge_started.elapsed());
        let diameter = if self.argb.is_some() {
            look.arrival_diameter()
        } else {
            f64::from(look.size.pixels(LANDING_SIZE))
        };
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
        let monitor = pick_monitor(&monitors, fallback, x, y);
        self.frame_interval = self.display_frame_interval(monitor);
        let area = Rect::from_bounds(monitor);
        let point = (f64::from(x), f64::from(y));
        let geometry = look::landing_rect(area, point, diameter);
        let rect = geometry.bounds();
        self.landing_frame.resize(rect.width, rect.height);
        self.landing_frame.arrival = Some(Arrival::at(geometry, point, 1.0));
        self.landing_frame.animate = self.animate && self.argb.is_some();
        if self.argb.is_some() {
            self.landing_frame.dark = true;
            self.landing_frame.opacity = 1.0;
            self.landing_frame.scale = 1.0;
            self.timeline
                .render_landing(&mut self.landing_frame, self.edge_started.elapsed());
            self.show(self.landing, rect)?;
            return self
                .argb
                .as_mut()?
                .blit(&self.connection, self.landing, &self.landing_frame);
        }
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
                &ChangeWindowAttributesAux::new()
                    .background_pixel(rgb_pixel(look.color.sample(0.0, true))),
            )
            .ok()?;
        self.connection
            .clear_area(false, self.landing, 0, 0, 0, 0)
            .ok()?;
        self.show(self.landing, mark)
    }

    fn show_glow(&mut self, display: Bounds, edge: Edge, progress: u16, look: Look) -> Option<()> {
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
        self.pace_edge(monitor);
        let along = if matches!(edge, Edge::Top | Edge::Bottom) {
            x
        } else {
            y
        };
        if self.argb.is_none() {
            let color = rgb_pixel(look.color.sample(0.0, true));
            self.connection
                .change_window_attributes(
                    self.glow,
                    &ChangeWindowAttributesAux::new().background_pixel(color),
                )
                .ok()?;
            self.connection
                .clear_area(false, self.glow, 0, 0, 0, 0)
                .ok()?;
        }
        let rect = if self.argb.is_some() {
            glow_rect(monitor, edge, along, 1000, look.size).map(|mut rect| {
                let depth = raster::OVERLAY_DEPTH.min(monitor.width).min(monitor.height);
                match edge {
                    Edge::Top => rect.height = depth,
                    Edge::Bottom => {
                        rect.height = depth;
                        rect.y = monitor.y.saturating_add_unsigned(monitor.height - depth);
                    }
                    Edge::Left => rect.width = depth,
                    Edge::Right => {
                        rect.width = depth;
                        rect.x = monitor.x.saturating_add_unsigned(monitor.width - depth);
                    }
                    Edge::Unspecified => {}
                }
                rect
            })
        } else {
            glow_rect(monitor, edge, along, progress, look.size)
        };
        let Some(rect) = rect else {
            self.timeline
                .end(End::Abandoned, self.edge_started.elapsed());
            return self.unmap(self.glow);
        };
        self.show(self.glow, rect)?;
        if let Some(argb) = &mut self.argb {
            self.edge_frame.resize(rect.width, rect.height);
            self.edge_frame.dark = true;
            self.edge_frame.animate = self.animate;
            self.edge_frame.opacity = 1.0;
            self.edge_frame.scale = 1.0;
            let horizontal = matches!(edge, Edge::Top | Edge::Bottom);
            let along = Along {
                origin: (f64::from(monitor.x), f64::from(monitor.y)),
                start: f64::from(if horizontal {
                    rect.x - monitor.x
                } else {
                    rect.y - monitor.y
                }),
                span: f64::from(if horizontal {
                    monitor.width
                } else {
                    monitor.height
                }),
                push: f64::from(if horizontal { x - rect.x } else { y - rect.y }),
            };
            let effect = EdgeEffect {
                look,
                edge,
                progress,
                along,
            };
            self.timeline.push(effect, self.edge_started.elapsed());
            self.timeline
                .render_edge(&mut self.edge_frame, self.edge_started.elapsed());
            argb.blit(&self.connection, self.glow, &self.edge_frame)?;
        }
        Some(())
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
