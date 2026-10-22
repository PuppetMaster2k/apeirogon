// SPDX-License-Identifier: GPL-3.0-or-later

//! X11 input backend: `XInput2` raw events capture, `XTest` injection.
//!
//! Two threads, each with its own connection: one captures and grabs, one
//! injects. They talk to the daemon through the channels in [`super`].
//!
//! Known limitation: bounds are the root window size at start; `RandR` changes
//! are not followed.

mod accumulate;
mod buttons;
mod capture;
mod inject;
mod keymap;

use tokio::sync::mpsc;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::xproto::{ConnectionExt as _, Window};
use x11rb::rust_connection::RustConnection;

use super::{Bounds, Capture, Injector, InputError};

/// Raw pointer and key events queued for the daemon.
const CAPTURE_EVENT_CAPACITY: usize = 1024;
const CAPTURE_CONTROL_CAPACITY: usize = 16;
const INJECT_COMMAND_CAPACITY: usize = 1024;
/// Position reports are superseded by the next one, so a short queue is enough.
const INJECT_POINTER_CAPACITY: usize = 64;

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
    let capture_thread = capture::CaptureThread::new(Session::connect()?)?;
    let inject_thread = inject::InjectThread::new(Session::connect()?)?;
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
        events: events_rx,
        control: control_tx,
    };
    let injector = Injector {
        bounds: inject_bounds,
        commands: commands_tx,
        pointer: pointer_rx,
    };
    Ok((capture, injector))
}
