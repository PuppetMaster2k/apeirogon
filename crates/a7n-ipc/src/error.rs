// SPDX-License-Identifier: GPL-3.0-or-later

use crate::ErrorKind;

/// Local transport, framing or version negotiation failure.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error(transparent)]
    Socket(#[from] a7n_platform::local_socket::SocketError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("invalid IPC JSON")]
    Json(#[from] serde_json::Error),
    #[error("IPC line exceeds the maximum length")]
    LineTooLong,
    #[error("IPC connection is closed")]
    Closed,
    #[error("IPC operation timed out")]
    Timeout,
    #[error("unsupported IPC version {received}; expected {expected}")]
    UnsupportedVersion { received: u32, expected: u32 },
    #[error("IPC handshake refused ({kind:?}): {message}")]
    HandshakeRefused { kind: ErrorKind, message: String },
    #[error("expected IPC Hello")]
    ExpectedHello,
}
