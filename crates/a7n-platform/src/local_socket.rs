// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-user local sockets. Endpoint permissions belong here, not in IPC framing.

use std::path::{Path, PathBuf};

pub use interprocess::local_socket::tokio::{Listener, Stream};

#[cfg(unix)]
#[path = "local_socket/unix.rs"]
mod imp;
#[cfg(windows)]
#[path = "local_socket/windows.rs"]
mod imp;

/// Explicit filesystem socket path on Unix, or pipe basename on Windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketLocation(PathBuf);

impl SocketLocation {
    /// Unix paths must be absolute and their parent directory must be private.
    #[must_use]
    pub fn new(location: impl Into<PathBuf>) -> Self {
        Self(location.into())
    }

    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

/// Endpoint setup or connection failure.
#[derive(Debug, thiserror::Error)]
pub enum SocketError {
    #[error("no per-user runtime/cache directory or username is available")]
    NoUserLocation,
    #[error("socket path must be absolute and at most {max_bytes} bytes: {path}")]
    InvalidPath { path: PathBuf, max_bytes: usize },
    #[error("socket directory must be a real directory owned by this user with mode 0700: {0}")]
    InsecureDirectory(PathBuf),
    #[error("refusing to replace a socket path that is not a socket owned by this user: {0}")]
    InsecureSocket(PathBuf),
    #[error("local socket operation timed out")]
    Timeout,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Select the current user's endpoint without creating it.
///
/// # Errors
/// Returns an error if the user's location cannot be determined or is too long.
pub fn default_location() -> Result<SocketLocation, SocketError> {
    imp::default_location()
}

/// Bind an endpoint accessible only to its owner.
///
/// # Errors
/// Refuses unsafe directories/files and live listeners; propagates OS failures.
pub async fn bind_user_only(location: &SocketLocation) -> Result<Listener, SocketError> {
    imp::bind_user_only(location).await
}

/// Connect to a per-user endpoint with a bounded connection attempt.
///
/// # Errors
/// Returns invalid-location, timeout or OS connection errors.
pub async fn connect(location: &SocketLocation) -> Result<Stream, SocketError> {
    imp::connect(location).await
}
