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
    use std::fmt::Write;

    use a7n_proto::MachineId;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub(super) fn serialize<S: Serializer>(
        id: &MachineId,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut hex = String::with_capacity(MachineId::LEN * 2);
        for byte in id.as_bytes() {
            write!(hex, "{byte:02x}").map_err(serde::ser::Error::custom)?;
        }
        serializer.serialize_str(&hex)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<MachineId, D::Error> {
        let hex = String::deserialize(deserializer)?;
        if hex.len() != MachineId::LEN * 2 || !hex.is_ascii() {
            return Err(D::Error::custom("machine id must contain 32 hex digits"));
        }
        let mut bytes = [0; MachineId::LEN];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
                .map_err(|_| D::Error::custom("machine id must contain 32 hex digits"))?;
        }
        MachineId::try_from(bytes.as_slice()).map_err(D::Error::custom)
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
