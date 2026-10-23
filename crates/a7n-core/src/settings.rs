// SPDX-License-Identifier: GPL-3.0-or-later

//! Convenience settings shared across a desk (PROTOCOL.md 13).

use std::collections::BTreeMap;

use a7n_proto::{MachineId, ProtoError, v1};

pub const SYNCED_KEYS: &[&str] = &[
    "input.speed",
    "input.natural_scrolling",
    "hotkeys.double_tap",
    "hotkeys.home",
    "hotkeys.lock",
    "hotkeys.jump",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub set_at: u64,
    pub by: MachineId,
    pub values: BTreeMap<String, String>,
}

impl TryFrom<&v1::SettingsSync> for Settings {
    type Error = ProtoError;

    fn try_from(wire: &v1::SettingsSync) -> Result<Self, Self::Error> {
        Ok(Self {
            set_at: wire.set_at,
            by: MachineId::try_from(wire.by.as_slice())?,
            values: wire
                .values
                .iter()
                .filter(|(key, _)| SYNCED_KEYS.contains(&key.as_str()))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        })
    }
}

impl Settings {
    #[must_use]
    pub fn to_proto(&self) -> v1::SettingsSync {
        v1::SettingsSync {
            on: true,
            set_at: self.set_at,
            by: self.by.as_bytes().to_vec(),
            values: self
                .values
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        }
    }
}

#[must_use]
pub fn newer(current: Option<&Settings>, incoming: &Settings) -> bool {
    // ponytail: clock skew decides competing intent; use logical clocks if this becomes a problem.
    current.is_none_or(|current| (incoming.set_at, incoming.by) > (current.set_at, current.by))
}

#[cfg(test)]
mod tests;
