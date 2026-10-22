// SPDX-License-Identifier: GPL-3.0-or-later

//! Channels shared by the daemon and future platform input threads.

use std::time::{Duration, Instant};

use a7n_proto::v1::InputEvent;
use tokio::sync::mpsc;

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
}

/// # Errors
/// Returns [`InputError::Unsupported`] until platform backends are implemented.
pub fn start() -> Result<(Capture, Injector), InputError> {
    Err(InputError::Unsupported)
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
