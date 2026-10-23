// SPDX-License-Identifier: GPL-3.0-or-later

//! Channels shared by the daemon and future platform input threads.

use std::time::{Duration, Instant};

use a7n_proto::v1::InputEvent;
use tokio::sync::mpsc;

#[cfg(any(windows, all(unix, not(target_os = "macos"))))]
mod scroll;
#[cfg(windows)]
mod windows;
#[cfg(all(unix, not(target_os = "macos")))]
mod x11;

pub const KEEPALIVE_INTERVAL: Duration = Duration::from_millis(500);
pub const DEADMAN_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug)]
pub enum Captured {
    Pointer { x: i32, y: i32, dx: i32, dy: i32 },
    Input(InputEvent),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureControl {
    Grab(bool),
    Keepalive,
}

/// While grabbed, all events still arrive here but do not reach the local OS.
#[derive(Debug)]
pub struct Capture {
    pub bounds: Bounds,
    pub events: mpsc::Receiver<Captured>,
    pub control: mpsc::Sender<CaptureControl>,
}

#[derive(Debug)]
pub enum Inject {
    Event(InputEvent),
    Motion { dx: i32, dy: i32 },
    Warp { x: i32, y: i32 },
}

#[derive(Debug)]
pub struct Injector {
    pub bounds: Bounds,
    pub commands: mpsc::Sender<Inject>,
    /// Absolute pointer position and raw delta after each motion or warp.
    pub pointer: mpsc::Receiver<Captured>,
}

#[derive(Debug, thiserror::Error)]
pub enum InputError {
    #[error("input capture and injection are not supported on this platform")]
    Unsupported,
    #[cfg(all(unix, not(target_os = "macos")))]
    #[error("cannot connect to the X server: {0}")]
    X11Connect(#[from] x11rb::errors::ConnectError),
    #[cfg(all(unix, not(target_os = "macos")))]
    #[error("X connection failed: {0}")]
    X11Connection(#[from] x11rb::errors::ConnectionError),
    #[cfg(all(unix, not(target_os = "macos")))]
    #[error("X request failed: {0}")]
    X11Reply(#[from] x11rb::errors::ReplyError),
    #[cfg(all(unix, not(target_os = "macos")))]
    #[error("the X server lacks the {0} extension")]
    MissingX11Extension(&'static str),
    #[cfg(any(windows, all(unix, not(target_os = "macos"))))]
    #[error("cannot start an input thread: {0}")]
    Thread(std::io::Error),
    #[cfg(windows)]
    #[error("Windows input failed: {0}")]
    Windows(#[from] std::io::Error),
}

/// # Errors
/// Returns [`InputError::Unsupported`] when no backend fits the session
/// (Wayland, no display, other platforms), or the backend's own error when it
/// cannot start.
pub fn start() -> Result<(Capture, Injector), InputError> {
    start_platform()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn start_platform() -> Result<(Capture, Injector), InputError> {
    let session_type = std::env::var("XDG_SESSION_TYPE").ok();
    let display_set = std::env::var_os("DISPLAY").is_some_and(|display| !display.is_empty());
    match select_backend(session_type.as_deref(), display_set) {
        Backend::X11 => x11::start(),
        Backend::None => Err(InputError::Unsupported),
    }
}

#[cfg(windows)]
fn start_platform() -> Result<(Capture, Injector), InputError> {
    windows::start()
}

#[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
fn start_platform() -> Result<(Capture, Injector), InputError> {
    Err(InputError::Unsupported)
}

#[cfg(all(unix, not(target_os = "macos")))]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Backend {
    X11,
    None,
}

/// `XWayland` cannot capture global input, so a Wayland session never gets the
/// X11 backend even when `DISPLAY` is set.
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn select_backend(session_type: Option<&str>, display_set: bool) -> Backend {
    if session_type == Some("wayland") || !display_set {
        return Backend::None;
    }
    Backend::X11
}

/// Backends check this independently so a stalled daemon cannot keep input grabbed.
#[derive(Debug)]
pub struct DeadMan {
    last: Instant,
}

impl DeadMan {
    #[must_use]
    pub fn new(now: Instant) -> Self {
        Self { last: now }
    }

    pub fn keepalive(&mut self, now: Instant) {
        self.last = now;
    }

    #[must_use]
    pub fn expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.last) >= DEADMAN_TIMEOUT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn select_backend_wayland_or_no_display_is_unsupported() {
        // Arrange
        let cases = [
            (Some("wayland"), true, Backend::None),
            (Some("x11"), false, Backend::None),
            (Some("x11"), true, Backend::X11),
            (None, true, Backend::X11),
        ];

        for (session_type, display_set, expected) in cases {
            // Act
            let backend = select_backend(session_type, display_set);

            // Assert
            assert_eq!(backend, expected, "{session_type:?} display={display_set}");
        }
    }

    #[test]
    fn expired_keepalive_missing_releases_at_timeout() {
        // Arrange
        let now = Instant::now();
        let mut deadman = DeadMan::new(now);
        deadman.keepalive(now + KEEPALIVE_INTERVAL);

        // Act
        let expired = [
            DEADMAN_TIMEOUT.saturating_sub(Duration::from_nanos(1)),
            DEADMAN_TIMEOUT,
        ]
        .map(|elapsed| deadman.expired(now + KEEPALIVE_INTERVAL + elapsed));

        // Assert
        assert_eq!(expired, [false, true]);
    }
}
