// SPDX-License-Identifier: GPL-3.0-or-later

//! Persistent store of paired peers (PROTOCOL.md 5 step 6).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use a7n_platform::fs::write_user_only;
use a7n_proto::{MachineId, Pin, ProtoError};
use serde::{Deserialize, Serialize};

use crate::identity::{self, IdentityError};

/// Why reading or writing the pin store failed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum PinStoreError {
    #[error("pin store file error: {0}")]
    Io(#[from] io::Error),
    #[error("pin store is not valid TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("pin store could not be encoded: {0}")]
    Encode(#[from] toml::ser::Error),
    #[error("pin store holds a malformed hex value")]
    BadHex,
    #[error(transparent)]
    Identity(#[from] IdentityError),
    #[error("certificate does not match the stored peer pin")]
    CertificateMismatch,
    #[error(transparent)]
    Proto(#[from] ProtoError),
}

/// One paired peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) machine_id: MachineId,
    pub(crate) pin: Pin,
    pub(crate) name: String,
    pub(crate) spki_der: Option<Vec<u8>>,
}

/// On-disk form of an [`Entry`]: ids and pins as lowercase hex.
#[derive(Serialize, Deserialize)]
struct EntryRecord {
    machine_id: String,
    pin: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    spki_der: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct FileRecord {
    #[serde(default)]
    peer: Vec<EntryRecord>,
}

/// Paired peers keyed by machine id, saved to one TOML file.
#[derive(Debug, Clone)]
pub(crate) struct PinStore {
    path: PathBuf,
    entries: BTreeMap<MachineId, Entry>,
}

impl PinStore {
    /// Loads the store at `path`; a missing file is an empty store.
    pub(crate) fn load(path: &Path) -> Result<Self, PinStoreError> {
        let mut entries = BTreeMap::new();
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error.into()),
        };
        for record in toml::from_str::<FileRecord>(&text)?.peer {
            let entry = Entry::try_from(record)?;
            entries.insert(entry.machine_id, entry);
        }
        Ok(Self {
            path: path.to_path_buf(),
            entries,
        })
    }

    /// Adds `entry`, replacing any entry with the same machine id.
    pub(crate) fn add(&mut self, entry: Entry) {
        self.entries.insert(entry.machine_id, entry);
    }

    /// The entry for `machine_id`, if paired.
    pub(crate) fn get(&self, machine_id: &MachineId) -> Option<&Entry> {
        self.entries.get(machine_id)
    }

    /// Caches a public key only after checking the pinned certificate and id.
    pub(crate) fn record_certificate(&mut self, cert_der: &[u8]) -> Result<(), PinStoreError> {
        let machine_id = identity::machine_id_of(cert_der)?;
        let pin = identity::pin_of(cert_der)?;
        let entry = self
            .entries
            .get_mut(&machine_id)
            .filter(|entry| entry.pin == pin)
            .ok_or(PinStoreError::CertificateMismatch)?;
        entry.spki_der = Some(identity::spki_of(cert_der)?);
        Ok(())
    }

    pub(crate) fn machine_ids(&self) -> impl Iterator<Item = MachineId> + '_ {
        self.entries.keys().copied()
    }

    /// Every paired peer's pin.
    pub(crate) fn pins(&self) -> impl Iterator<Item = Pin> + '_ {
        self.entries.values().map(|entry| entry.pin)
    }

    /// Removes and returns the entry for `machine_id` (revocation).
    pub(crate) fn remove(&mut self, machine_id: &MachineId) -> Option<Entry> {
        self.entries.remove(machine_id)
    }

    /// Writes the store to disk, user-only.
    pub(crate) fn save(&self) -> Result<(), PinStoreError> {
        let record = FileRecord {
            peer: self.entries.values().map(EntryRecord::from).collect(),
        };
        write_user_only(&self.path, toml::to_string(&record)?.as_bytes())?;
        Ok(())
    }
}

impl From<&Entry> for EntryRecord {
    fn from(entry: &Entry) -> Self {
        Self {
            machine_id: entry.machine_id.to_string(),
            pin: hex_encode(entry.pin.as_bytes()),
            name: entry.name.clone(),
            spki_der: entry.spki_der.as_deref().map(hex_encode),
        }
    }
}

impl TryFrom<EntryRecord> for Entry {
    type Error = PinStoreError;

    fn try_from(record: EntryRecord) -> Result<Self, Self::Error> {
        Ok(Self {
            machine_id: record.machine_id.parse()?,
            pin: Pin::try_from(&hex_decode(&record.pin)?[..])?,
            name: record.name,
            spki_der: record.spki_der.as_deref().map(hex_decode).transpose()?,
        })
    }
}

/// Lowercase hex of `bytes`.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        // Writing to a String cannot fail.
        let _ = write!(out, "{byte:02x}");
        out
    })
}

fn hex_decode(text: &str) -> Result<Vec<u8>, PinStoreError> {
    if !text.bytes().all(|byte| byte.is_ascii_hexdigit()) || text.len() % 2 != 0 {
        return Err(PinStoreError::BadHex);
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).map_err(|_| PinStoreError::BadHex))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(seed: u8) -> Entry {
        Entry {
            machine_id: MachineId::try_from(&[seed; 16][..]).unwrap(),
            pin: Pin::try_from(&[seed; 32][..]).unwrap(),
            name: format!("peer-{seed}"),
            spki_der: None,
        }
    }

    #[test]
    fn pin_store_save_and_load_keeps_entries() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pins.toml");
        let mut store = PinStore::load(&path).unwrap();
        store.add(entry(1));
        store.save().unwrap();

        // Act
        let reloaded = PinStore::load(&path).unwrap();

        // Assert
        assert_eq!(reloaded.get(&entry(1).machine_id), Some(&entry(1)));
    }

    #[test]
    fn pin_store_remove_then_reload_forgets_peer() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pins.toml");
        let mut store = PinStore::load(&path).unwrap();
        store.add(entry(1));
        store.save().unwrap();
        store.remove(&entry(1).machine_id);
        store.save().unwrap();

        // Act
        let reloaded = PinStore::load(&path).unwrap();

        // Assert
        assert!(reloaded.get(&entry(1).machine_id).is_none());
    }

    #[test]
    fn pin_store_load_bad_pin_length_is_rejected() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pins.toml");
        let short_pin = "ab".repeat(31);
        let id = "01".repeat(16);
        let text =
            format!("[[peer]]\nmachine_id = \"{id}\"\npin = \"{short_pin}\"\nname = \"x\"\n");
        fs::write(&path, text).unwrap();

        // Act
        let result = PinStore::load(&path);

        // Assert
        assert!(matches!(
            result,
            Err(PinStoreError::Proto(ProtoError::BadPin(31)))
        ));
    }
    #[test]
    fn load_old_pin_file_keeps_peer_without_spki() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pins.toml");
        fs::write(
            &path,
            format!(
                "[[peer]]\nmachine_id = \"{}\"\npin = \"{}\"\nname = \"old\"\n",
                "01".repeat(16),
                "01".repeat(32)
            ),
        )
        .unwrap();

        // Act
        let store = PinStore::load(&path).unwrap();

        // Assert
        assert!(store.get(&entry(1).machine_id).unwrap().spki_der.is_none());
    }

    #[test]
    fn record_certificate_wrong_pin_rejects_public_key() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let identity = identity::load_or_create(dir.path()).unwrap();
        let mut store = PinStore::load(&dir.path().join("pins.toml")).unwrap();
        store.add(Entry {
            machine_id: identity.machine_id,
            pin: entry(1).pin,
            name: "peer".into(),
            spki_der: None,
        });

        // Act
        let result = store.record_certificate(&identity.cert_der);

        // Assert
        assert!(matches!(result, Err(PinStoreError::CertificateMismatch)));
        assert!(store.get(&identity.machine_id).unwrap().spki_der.is_none());
    }
}
