// SPDX-License-Identifier: GPL-3.0-or-later

//! X11 input backend: `XInput2` raw events capture, `XTest` injection.
//!
//! Two threads, each with its own connection: one captures and grabs, one
//! injects. They talk to the daemon through the channels in [`super`].
//!
//! Known limitation: bounds are the root window size at start; `RandR` changes
//! are not followed.

mod buttons;
mod capture;
mod inject;
mod keymap;

use tokio::sync::mpsc;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::xproto::{ConnectionExt as _, Window};
use x11rb::rust_connection::RustConnection;

use super::{
    Bounds, CAPTURE_CONTROL_CAPACITY, CAPTURE_EVENT_CAPACITY, Capture, INJECT_COMMAND_CAPACITY,
    INJECT_POINTER_CAPACITY, Injector, InputError,
};

struct Session {
    conn: RustConnection,
    root: Window,
    bounds: Bounds,
}

impl Session {
    fn connect() -> Result<Self, InputError> {
        let (conn, screen_num) = x11rb::connect(None)?;
        let screen = &conn.setup().roots[screen_num];
        let bounds = Bounds {
            x: 0,
            y: 0,
            width: u32::from(screen.width_in_pixels),
            height: u32::from(screen.height_in_pixels),
        };
        let root = screen.root;
        Ok(Self { conn, root, bounds })
    }

    fn require_extension(&self, name: &'static str) -> Result<(), InputError> {
        match self.conn.extension_information(name)? {
            Some(_) => Ok(()),
            None => Err(InputError::MissingX11Extension(name)),
        }
    }

    /// Absolute pointer position on the root window.
    fn pointer_position(&self) -> Result<(i32, i32), x11rb::errors::ReplyError> {
        let reply = self.conn.query_pointer(self.root)?.reply()?;
        Ok((i32::from(reply.root_x), i32::from(reply.root_y)))
    }
}

pub(super) fn start() -> Result<(Capture, Injector), InputError> {
    let capture_session = Session::connect()?;
    let inject_session = Session::connect()?;
    let capture_monitors = monitors(&capture_session.conn, capture_session.root)
        .unwrap_or_else(|| vec![capture_session.bounds]);
    let inject_monitors = monitors(&inject_session.conn, inject_session.root)
        .unwrap_or_else(|| vec![inject_session.bounds]);
    let capture_thread = capture::CaptureThread::new(capture_session)?;
    let inject_thread = inject::InjectThread::new(inject_session)?;
    let capture_bounds = capture_thread.bounds();
    let inject_bounds = inject_thread.bounds();

    let (events_tx, events_rx) = mpsc::channel(CAPTURE_EVENT_CAPACITY);
    let (control_tx, control_rx) = mpsc::channel(CAPTURE_CONTROL_CAPACITY);
    let (commands_tx, commands_rx) = mpsc::channel(INJECT_COMMAND_CAPACITY);
    let (pointer_tx, pointer_rx) = mpsc::channel(INJECT_POINTER_CAPACITY);

    // If the second spawn fails the first thread sees its channel close and
    // exits, ungrabbing on the way out.
    capture_thread.spawn(events_tx, control_rx)?;
    inject_thread.spawn(commands_rx, pointer_tx)?;

    let capture = Capture {
        bounds: capture_bounds,
        monitors: capture_monitors.into(),
        notch: None,
        events: events_rx,
        control: control_tx,
    };
    let injector = Injector {
        bounds: inject_bounds,
        monitors: inject_monitors.into(),
        notch: None,
        commands: commands_tx,
        pointer: pointer_rx,
    };
    Ok((capture, injector))
}

pub(crate) fn monitors(connection: &RustConnection, root: Window) -> Option<Vec<Bounds>> {
    use x11rb::protocol::randr::ConnectionExt as _;
    let reply = connection
        .randr_get_monitors(root, true)
        .ok()?
        .reply()
        .ok()?;
    let monitors: Vec<_> = reply
        .monitors
        .iter()
        .filter(|m| m.width > 0 && m.height > 0)
        .map(|m| Bounds {
            x: i32::from(m.x),
            y: i32::from(m.y),
            width: u32::from(m.width),
            height: u32::from(m.height),
        })
        .collect();
    (!monitors.is_empty()).then_some(monitors)
}

/// The refresh rate of the `RandR` output showing exactly `monitor`, or `None`
/// when no active mode matches or any request fails.
pub(crate) fn refresh_hz(
    connection: &RustConnection,
    root: Window,
    monitor: Bounds,
) -> Option<f64> {
    use x11rb::protocol::randr::ConnectionExt as _;
    let resources = connection
        .randr_get_screen_resources_current(root)
        .ok()?
        .reply()
        .ok()?;
    let mode = resources.crtcs.iter().find_map(|crtc| {
        let info = connection
            .randr_get_crtc_info(*crtc, resources.config_timestamp)
            .ok()?
            .reply()
            .ok()?;
        let same_area = (i32::from(info.x), i32::from(info.y)) == (monitor.x, monitor.y)
            && (u32::from(info.width), u32::from(info.height)) == (monitor.width, monitor.height);
        (same_area && info.mode != 0).then_some(info.mode)
    })?;
    let mode = resources.modes.iter().find(|m| m.id == mode)?;
    let total = u32::from(mode.htotal) * u32::from(mode.vtotal);
    (total > 0).then(|| f64::from(mode.dot_clock) / f64::from(total))
}
