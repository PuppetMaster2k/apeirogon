// SPDX-License-Identifier: GPL-3.0-or-later

//! Visual and haptic feedback while the pointer pushes against a screen edge
//! and after it lands on this machine.
//!
//! # Backend contract
//!
//! - [`start`] runs once at daemon startup on a tokio thread. A backend creates
//!   its channel with [`channel`], moves the receiver to its own OS UI thread
//!   and returns the [`Feedback`]. It returns `None` when it cannot start;
//!   it logs nothing and never panics.
//! - The UI thread owns every window or OS object. Commands arrive on a
//!   bounded channel (read it with `blocking_recv`). The sender never blocks
//!   and drops commands when the thread is behind, so the backend must treat
//!   every command as absolute state, never as a delta.
//! - Never steal focus, never take input, and make every window click-through
//!   and invisible to screen capture where the OS allows it. Nothing may
//!   appear in the task bar, dock or window switcher.
//! - Honouring the OS reduce-motion setting is the backend's job: with it on,
//!   show the same state without animation (for example a static edge strip
//!   and a landing marker that does not pulse).
//! - Never log positions or progress.
//! - Commands:
//!   - [`Command::Edge`]: draw the push glow along `edge` of the display
//!     `display` (desktop coordinates; the daemon sends the bounds of the
//!     whole local desktop it captures or injects in, so a backend that knows
//!     per-monitor bounds clamps the glow to the monitor containing the
//!     pointer). `progress` runs 0..=1000 per-mille of the resistance that
//!     is built up; the glow grows with it. Repeated commands update it.
//!   - [`Command::Clear`]: remove the glow at once (push ended, crossing,
//!     pause, focus moved or the setting turned off).
//!   - [`Command::Landing`]: briefly mark the desktop point `(x, y)` where the
//!     pointer landed, then fade out on its own.
//!   - [`Command::LandingAtCursor`]: mark the current OS cursor position.
//!   - [`Command::Tick`]: one trackpad haptic tick. The daemon only sends it
//!     when the user enabled haptics; backends without haptics ignore it.

use a7n_proto::v1::Edge;
use tokio::sync::mpsc;

use crate::input::Bounds;

mod look;
pub use look::{Color, Landing, Look, Size, Style};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "macos", test))]
mod macos_geometry;
#[cfg(windows)]
mod windows;
#[cfg(all(test, not(windows)))]
#[path = "feedback/windows/geometry.rs"]
mod windows_geometry;
#[cfg(all(unix, not(target_os = "macos")))]
mod x11;

/// Commands the daemon may queue before the backend catches up.
const COMMAND_CAPACITY: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Edge {
        display: Bounds,
        edge: Edge,
        progress: u16,
        look: Look,
    },
    Clear,
    Landing {
        x: i32,
        y: i32,
        look: Look,
    },
    LandingAtCursor {
        look: Look,
    },
    Tick,
}

/// Handle to a running feedback backend.
#[derive(Debug, Clone)]
pub struct Feedback {
    commands: mpsc::Sender<Command>,
}

impl Feedback {
    /// Queues a command. A full or closed queue drops it: the input path must
    /// never wait for the UI.
    pub fn send(&self, command: Command) {
        let _ = self.commands.try_send(command);
    }
}

/// A feedback handle and the receiver its backend thread reads.
#[must_use]
pub fn channel() -> (Feedback, mpsc::Receiver<Command>) {
    let (commands, receiver) = mpsc::channel(COMMAND_CAPACITY);
    (Feedback { commands }, receiver)
}

/// Whether this session can show feedback: Windows, macOS and X11.
#[must_use]
pub fn supported() -> bool {
    #[cfg(any(windows, target_os = "macos"))]
    {
        true
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        crate::input::detect_backend() == crate::input::Backend::X11
    }
    #[cfg(not(any(windows, unix)))]
    {
        false
    }
}

/// Starts the backend, or returns `None` when this session has none.
#[must_use]
pub fn start() -> Option<Feedback> {
    supported().then(platform_start).flatten()
}

/// Whether this OS can play haptic ticks.
#[must_use]
pub const fn haptics_supported() -> bool {
    cfg!(target_os = "macos")
}

#[cfg(target_os = "macos")]
use macos::start as platform_start;
#[cfg(windows)]
use windows::start as platform_start;
#[cfg(all(unix, not(target_os = "macos")))]
use x11::start as platform_start;
#[cfg(not(any(windows, unix)))]
fn platform_start() -> Option<Feedback> {
    None
}
