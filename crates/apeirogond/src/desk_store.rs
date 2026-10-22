// SPDX-License-Identifier: GPL-3.0-or-later

//! Durable desk records and their effects on paired pins (PROTOCOL.md 6).

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use a7n_core::desk::{
    DeskError, DeskState, MergeOutcome, member_signing_input, revocation_signing_input,
};
use a7n_platform::fs::write_user_only;
use a7n_proto::{
    MachineId, limits,
    v1::{Desk, DeskMember, DeskRevocation},
};
use prost::Message;
use rcgen::PublicKeyData;

use crate::identity::{self, Identity, IdentityError};
use crate::pin_store::{Entry, PinStore, PinStoreError};

#[cfg(test)]
mod tests;

#[derive(Debug, thiserror::Error)]
pub(crate) enum DeskStoreError {
    #[error("desk file error: {0}")]
    Io(#[from] io::Error),
    #[error("desk file is not valid protobuf: {0}")]
    Decode(#[from] prost::DecodeError),
    #[error(transparent)]
    Desk(#[from] DeskError),
    #[error(transparent)]
    Identity(#[from] IdentityError),
    #[error(transparent)]
    Pins(#[from] PinStoreError),
    #[error("local desk edit was rejected")]
    Rejected,
}

#[derive(Debug)]
pub(crate) struct DeskStore {
    path: PathBuf,
    state: DeskState,
}

impl DeskStore {
    /// Load `desk.pb` next to `pins.toml`, reconciling interrupted pin writes.
    pub(crate) fn load(
        path: &Path,
        own_id: MachineId,
        pins: &mut PinStore,
    ) -> Result<Self, DeskStoreError> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Self {
                    path: path.to_path_buf(),
                    state: DeskState::new(own_id),
                });
            }
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        file.take((limits::CONTROL_FRAME_MAX + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > limits::CONTROL_FRAME_MAX {
            return Err(DeskError::Capacity.into());
        }
        let desk = Desk::decode(bytes.as_slice())?;
        let state = DeskState::restore(own_id, &desk)?;
        let store = Self {
            path: path.to_path_buf(),
            state,
        };
        // Desk is committed first; replay it if a crash left the pins behind.
        for record in &desk.revocations {
            let id = MachineId::try_from(record.id.as_slice()).map_err(DeskError::from)?;
            if !store.state.members().any(|member| member.id() == id) {
                pins.remove(&id);
            }
        }
        for member in store.state.members() {
            store_member(pins, member);
        }
        pins.save()?;
        Ok(store)
    }

    pub(crate) fn to_desk(&self) -> Desk {
        self.state.to_desk()
    }

    /// Merge against the trust and certificate keys known before this message.
    pub(crate) fn merge(
        &mut self,
        identity: &Identity,
        pins: &mut PinStore,
        incoming: &Desk,
    ) -> Result<MergeOutcome, DeskStoreError> {
        let (staged, outcome) = self.prepare(identity, pins, incoming)?;
        self.persist(staged)?;
        Self::apply(pins, &outcome)?;
        Ok(outcome)
    }

    fn prepare(
        &self,
        identity: &Identity,
        pins: &PinStore,
        incoming: &Desk,
    ) -> Result<(DeskState, MergeOutcome), DeskStoreError> {
        let mut staged = self.state.clone();
        let own_spki = identity.key.subject_public_key_info();
        let outcome = staged.merge(
            incoming,
            &|id| id == identity.machine_id || pins.get(&id).is_some(),
            |id, message, signature| {
                let spki = if id == identity.machine_id {
                    Some(own_spki.as_slice())
                } else {
                    pins.get(&id).and_then(|entry| entry.spki_der.as_deref())
                };
                spki.is_some_and(|spki| identity::verify(spki, message, signature))
            },
        );
        if outcome.capacity_reached {
            return Err(DeskError::Capacity.into());
        }
        Ok((staged, outcome))
    }

    fn persist(&mut self, staged: DeskState) -> Result<(), DeskStoreError> {
        let bytes = staged.to_desk().encode_to_vec();
        if bytes.len() > limits::CONTROL_FRAME_MAX {
            return Err(DeskError::Capacity.into());
        }
        write_user_only(&self.path, &bytes)?;
        self.state = staged;
        Ok(())
    }

    /// Apply a committed delta. Keep memory revoked even if the pin write fails.
    pub(crate) fn apply(pins: &mut PinStore, outcome: &MergeOutcome) -> Result<(), PinStoreError> {
        for id in &outcome.revoked {
            pins.remove(id);
        }
        let mut staged = pins.clone();
        for member in &outcome.admitted {
            store_member(&mut staged, member);
        }
        staged.save()?;
        *pins = staged;
        Ok(())
    }

    /// Sign and persist the newcomer before acknowledging successful pairing.
    pub(crate) fn admit(
        &mut self,
        identity: &Identity,
        pins: &mut PinStore,
        entry: &Entry,
        now: u64,
    ) -> Result<DeskMember, DeskStoreError> {
        let input = member_signing_input(
            entry.machine_id,
            entry.pin,
            &entry.name,
            identity.machine_id,
            now,
        )?;
        let member = DeskMember {
            id: entry.machine_id.as_bytes().to_vec(),
            pin: entry.pin.as_bytes().to_vec(),
            name: entry.name.clone(),
            introduced_by: identity.machine_id.as_bytes().to_vec(),
            added_at: now,
            signature: identity::sign(identity, &input)?,
        };
        let (staged, outcome) = self.prepare(
            identity,
            pins,
            &Desk {
                members: vec![member.clone()],
                revocations: Vec::new(),
            },
        )?;
        if !staged
            .members()
            .any(|active| DeskMember::from(active) == member)
        {
            return Err(DeskStoreError::Rejected);
        }
        self.persist(staged)?;
        let mut staged_pins = pins.clone();
        staged_pins.add(entry.clone());
        Self::apply(&mut staged_pins, &outcome)?;
        *pins = staged_pins;
        Ok(member)
    }

    /// Sign a revocation and remove its pin; #23 disconnects the live link.
    pub(crate) fn revoke(
        &mut self,
        identity: &Identity,
        pins: &mut PinStore,
        id: MachineId,
        now: u64,
    ) -> Result<DeskRevocation, DeskStoreError> {
        let input = revocation_signing_input(id, identity.machine_id, now);
        let revocation = DeskRevocation {
            id: id.as_bytes().to_vec(),
            revoked_by: identity.machine_id.as_bytes().to_vec(),
            revoked_at: now,
            signature: identity::sign(identity, &input)?,
        };
        let (staged, outcome) = self.prepare(
            identity,
            pins,
            &Desk {
                members: Vec::new(),
                revocations: vec![revocation.clone()],
            },
        )?;
        if !staged
            .to_desk()
            .revocations
            .iter()
            .any(|stored| stored == &revocation)
        {
            return Err(DeskStoreError::Rejected);
        }
        self.persist(staged)?;
        Self::apply(pins, &outcome)?;
        Ok(revocation)
    }
}

fn store_member(pins: &mut PinStore, member: &a7n_core::desk::Member) {
    let spki_der = pins
        .get(&member.id())
        .filter(|entry| entry.pin == member.pin())
        .and_then(|entry| entry.spki_der.clone());
    pins.add(Entry {
        machine_id: member.id(),
        pin: member.pin(),
        name: member.name().to_owned(),
        spki_der,
    });
}
