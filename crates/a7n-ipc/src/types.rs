// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::SocketAddr;
use std::path::PathBuf;

use a7n_core::pairing::PairingCode;
use a7n_proto::MachineId;
use serde::{Deserialize, Serialize};

use crate::FirewallProblem;
use crate::config::ClipboardPolicy;

/// The config value to collect from local key capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyKind {
    Trigger,
    Chord,
    Modifiers,
    Single,
}

/// Commands accepted after the version handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    ReloadConfig,
    RecordHotkey {
        kind: HotkeyKind,
    },
    Shutdown,
    SetPaused {
        paused: bool,
    },
    Switch {
        #[serde(with = "machine_id")]
        machine: MachineId,
    },
    Wake {
        #[serde(with = "machine_id")]
        machine: MachineId,
    },
    Status,
    SendFiles {
        #[serde(with = "machine_id")]
        peer: MachineId,
        paths: Vec<PathBuf>,
    },
    FilesAccept {
        #[serde(with = "machine_id")]
        peer: MachineId,
        offer_id: u64,
    },
    FilesReject {
        #[serde(with = "machine_id")]
        peer: MachineId,
    },
    CancelTransfer {
        id: u64,
    },
    ClipboardStatus,
    ClipboardAccept {
        #[serde(with = "machine_id")]
        machine: MachineId,
        generation: u64,
    },
    ClipboardReject,
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
    /// Ask the daemon to prompt for a macOS permission and open its settings pane.
    RequestPermission {
        permission: Permission,
    },
}

/// Responses to one command. Pairing codes retain leading zeroes and redact Debug.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Response {
    Status(StatusInfo),
    Recorded {
        value: String,
    },
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
    FilesOffered {
        offer_id: u64,
        skipped_symlinks: usize,
    },
    Waking {
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
    NoSpace,
    Internal,
}

/// A macOS privacy permission the daemon needs to share input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Accessibility,
    InputMonitoring,
}

impl From<a7n_platform::permissions::Permission> for Permission {
    fn from(permission: a7n_platform::permissions::Permission) -> Self {
        match permission {
            a7n_platform::permissions::Permission::Accessibility => Self::Accessibility,
            a7n_platform::permissions::Permission::InputMonitoring => Self::InputMonitoring,
        }
    }
}

impl From<Permission> for a7n_platform::permissions::Permission {
    fn from(permission: Permission) -> Self {
        match permission {
            Permission::Accessibility => Self::Accessibility,
            Permission::InputMonitoring => Self::InputMonitoring,
        }
    }
}

impl std::fmt::Display for Permission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&a7n_platform::permissions::Permission::from(*self), f)
    }
}

/// Snapshot of the daemon's own identity and known peers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusInfo {
    /// None when input works, otherwise the reason it is unavailable.
    pub input: Option<String>,
    pub held_clipboard: Option<HeldClipboard>,
    pub transfers: Vec<FileTransfer>,
    pub held_files: Vec<HeldFiles>,
    /// Most recent ended sends, oldest first; session state only.
    pub finished_sends: Vec<FinishedSend>,
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
    /// macOS permissions the daemon still needs; empty elsewhere.
    pub missing_permissions: Vec<Permission>,
    /// Windows Firewall problems that block links; empty elsewhere.
    pub firewall: Vec<FirewallProblem>,
}

/// Coarse file offer metadata; paths and names are never exposed here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeldFiles {
    pub offer_id: u64,
    #[serde(with = "machine_id")]
    pub peer: MachineId,
    pub files: u32,
    pub top_level_entries: u32,
    pub total_size: u64,
}

/// Progress for an active transfer. Ids are local to this daemon session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileTransfer {
    pub id: u64,
    /// The local offer a send belongs to; `None` for receives.
    pub offer_id: Option<u64>,
    #[serde(with = "machine_id")]
    pub peer: MachineId,
    pub direction: TransferDirection,
    pub files_done: u32,
    pub files_total: u32,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

/// How a send transfer ended. Link loss is not an outcome: the offer stays live and resumes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinishedSend {
    pub offer_id: u64,
    #[serde(with = "machine_id")]
    pub peer: MachineId,
    pub outcome: SendOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SendOutcome {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferDirection {
    Send,
    Receive,
}

/// Clipboard availability and metadata; never includes contents or source apps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClipboardInfo {
    pub available: bool,
    pub current: Option<ClipboardCopy>,
    pub held: Option<HeldClipboard>,
    pub peers: Vec<PeerClipboard>,
}

/// Metadata awaiting local confirmation; never includes contents or raw MIME types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeldClipboard {
    pub generation: u64,
    #[serde(with = "machine_id")]
    pub machine: MachineId,
    pub name: String,
    pub items: u32,
    pub size: u64,
    pub kinds: Vec<ClipboardKind>,
}

/// Coarse content categories safe to show in confirmation prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardKind {
    Text,
    Image,
}

impl std::fmt::Display for ClipboardKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Text => "text",
            Self::Image => "image",
        })
    }
}

impl HeldClipboard {
    /// Comma-separated kinds for prompts, e.g. `text, image`.
    #[must_use]
    pub fn kinds_label(&self) -> String {
        self.kinds
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
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
    pub blocked: u16,
}

/// A placed rectangle with its display name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacedMachine {
    pub screen_mm: Option<(u32, u32)>,
    #[serde(with = "machine_id")]
    pub machine: MachineId,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub blocked: u16,
}

/// A known machine without a canvas rectangle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnplacedMachine {
    pub screen_mm: Option<(u32, u32)>,
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

/// The last known reason a peer could not establish or keep its link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkProblem {
    Unreachable,
    Refused,
    AuthFailed,
    VersionMismatch { older: Option<String> },
    Silent,
}

/// One known peer, whether connected or offline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerInfo {
    #[serde(with = "machine_id")]
    pub id: MachineId,
    pub name: String,
    pub connected: bool,
    pub problem: Option<LinkProblem>,
    pub rtt_ms: Option<u64>,
    /// A validated hardware address has been remembered; never expose the address.
    pub can_wake: bool,
    /// A wake request is awaiting this peer's link.
    pub waking: bool,
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
