// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::SocketAddr;

use a7n_core::pairing::PairingCode;
use a7n_proto::MachineId;
use serde::{Deserialize, Serialize};

use crate::config::ClipboardPolicy;

/// Commands accepted after the version handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    ReloadConfig,
    Shutdown,
    SetPaused {
        paused: bool,
    },
    Switch {
        #[serde(with = "machine_id")]
        machine: MachineId,
    },
    Status,
    ClipboardStatus,
    GetLayout,
    SetLayout {
        placed: Vec<LayoutPlacement>,
    },
    PairHost,
    PairJoin {
        addr: SocketAddr,
        #[serde(with = "pairing_code")]
        code: PairingCode,
    },
    Unpair {
        #[serde(with = "machine_id")]
        machine: MachineId,
    },
    Connect {
        addr: SocketAddr,
    },
}

/// Responses to one command. Pairing codes retain leading zeroes and redact Debug.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Response {
    Status(StatusInfo),
    Clipboard(ClipboardInfo),
    Layout(LayoutInfo),
    PairCode {
        #[serde(with = "pairing_code")]
        code: PairingCode,
        expires_in_secs: u64,
    },
    Paired {
        #[serde(with = "machine_id")]
        machine: MachineId,
        name: String,
    },
    Done,
    Error {
        kind: ErrorKind,
        message: String,
    },
}

/// Stable error categories for clients; `message` is for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    InvalidConfig,
    UnsupportedVersion,
    InvalidRequest,
    InvalidLayout,
    NotPaired,
    NotConnected,
    PairingRefused,
    Busy,
    Internal,
}

/// Snapshot of the daemon's own identity and known peers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusInfo {
    /// None when input works, otherwise the reason it is unavailable.
    pub input: Option<String>,
    pub pid: u32,
    /// Outgoing input sharing is suspended for this daemon session.
    pub paused: bool,
    /// Where this machine currently sends its physical input.
    #[serde(with = "machine_id")]
    pub focused: MachineId,
    #[serde(with = "machine_id")]
    pub machine: MachineId,
    pub name: String,
    pub peers: Vec<PeerInfo>,
    pub nearby: Vec<NearbyMachine>,
}

/// Clipboard availability and metadata; never includes contents or source apps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClipboardInfo {
    pub available: bool,
    pub current: Option<ClipboardCopy>,
    pub peers: Vec<PeerClipboard>,
}

/// Metadata of the current announced copy, without formats or payloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClipboardCopy {
    #[serde(with = "machine_id")]
    pub origin: MachineId,
    pub origin_name: Option<String>,
    pub generation: u64,
    pub items: u32,
    pub size: u64,
    pub local: bool,
}

/// Local clipboard policy and link state for one paired peer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerClipboard {
    #[serde(with = "machine_id")]
    pub machine: MachineId,
    pub name: String,
    pub policy: ClipboardPolicy,
    pub linked: bool,
}

/// Desk canvas and known machines that have not been placed yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutInfo {
    pub placed: Vec<PlacedMachine>,
    pub unplaced: Vec<UnplacedMachine>,
    pub lost_edit: bool,
}

/// One rectangle submitted by the layout editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutPlacement {
    #[serde(with = "machine_id")]
    pub machine: MachineId,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// A placed rectangle with its display name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacedMachine {
    #[serde(with = "machine_id")]
    pub machine: MachineId,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// A known machine without a canvas rectangle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnplacedMachine {
    #[serde(with = "machine_id")]
    pub machine: MachineId,
    pub name: String,
}

/// An unpaired machine discovered on the local network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NearbyMachine {
    #[serde(with = "machine_id")]
    pub id: MachineId,
    pub addr: SocketAddr,
}

/// One known peer, whether connected or offline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerInfo {
    #[serde(with = "machine_id")]
    pub id: MachineId,
    pub name: String,
    pub connected: bool,
    pub rtt_ms: Option<u64>,
}

mod machine_id {
    use a7n_proto::MachineId;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub(super) fn serialize<S: Serializer>(
        id: &MachineId,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_str(id)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<MachineId, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

mod pairing_code {
    use a7n_core::pairing::PairingCode;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub(super) fn serialize<S: Serializer>(
        code: &PairingCode,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_str(code)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<PairingCode, D::Error> {
        let code = String::deserialize(deserializer)?;
        PairingCode::parse(&code).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests;
