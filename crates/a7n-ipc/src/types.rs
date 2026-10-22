// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::SocketAddr;

use a7n_core::pairing::PairingCode;
use a7n_proto::MachineId;
use serde::{Deserialize, Serialize};

/// Commands accepted after the version handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Status,
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
    UnsupportedVersion,
    InvalidRequest,
    NotPaired,
    PairingRefused,
    Busy,
    Internal,
}

/// Snapshot of the daemon's own identity and known peers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusInfo {
    #[serde(with = "machine_id")]
    pub machine: MachineId,
    pub name: String,
    pub peers: Vec<PeerInfo>,
    pub nearby: Vec<NearbyMachine>,
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
