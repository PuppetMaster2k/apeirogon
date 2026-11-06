// SPDX-License-Identifier: GPL-3.0-or-later
#![forbid(unsafe_code)]

//! Versioned JSON-lines API shared by the daemon, CLI and GUI.
//!
//! Each connection starts with `{"ipc_version":10}` in both directions, then
//! exchanges one request and one response at a time. Messages use a `type` tag
//! and `snake_case` variant names, with fields alongside the tag. Status fields
//! are flattened into the status response. Machine ids are 32 hex characters.
//! A handshake error is a regular `Response::Error`, followed by closure.

pub mod config;
mod connection;
pub mod daemon;
mod error;
mod format;
mod framing;
pub mod layout;
mod types;

pub use a7n_core::layout::{Edge, Placement, SharedEdge, shared_edges, third_at, validate};
pub use a7n_core::pairing::{InvalidPairingCode, PairingCode};
pub use a7n_platform::local_socket::{SocketLocation, default_location};
pub use a7n_proto::MachineId;
pub use a7n_proto::limits::DEFAULT_UDP_PORT;
pub use connection::{IpcClient, IpcConnection, IpcServer};
pub use error::IpcError;
pub use format::format_size;
pub use types::{
    ClipboardCopy, ClipboardInfo, ClipboardKind, ErrorKind, FileTransfer, FinishedSend,
    HeldClipboard, HeldFiles, LayoutInfo, LayoutPlacement, NearbyMachine, PeerClipboard, PeerInfo,
    Permission, PlacedMachine, Request, Response, SendOutcome, StatusInfo, TransferDirection,
    UnplacedMachine,
};

/// Local API major version, independent of the peer protobuf protocol.
pub const IPC_VERSION: u32 = 10;
/// Maximum JSON payload bytes per line, excluding the terminating newline.
pub const MAX_LINE_LENGTH: usize = 64 * 1024;
