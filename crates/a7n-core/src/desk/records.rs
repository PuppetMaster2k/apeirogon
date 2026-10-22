// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_proto::{
    MachineId, Pin, ProtoError, limits,
    v1::{DeskMember, DeskRevocation},
};

const MEMBER_DOMAIN: &[u8] = b"a7n desk member v1\0";
const REVOCATION_DOMAIN: &[u8] = b"a7n desk revoke v1\0";
pub(super) const SIGNATURE_LEN: usize = 64;

/// A malformed desk record or a full persisted desk.
#[derive(Debug, thiserror::Error)]
pub enum DeskError {
    #[error(transparent)]
    Proto(#[from] ProtoError),
    #[error(
        "peer name must contain 1..={} Unicode code points",
        limits::PEER_NAME_MAX
    )]
    BadName,
    #[error("desk signature must contain {SIGNATURE_LEN} bytes")]
    BadSignature,
    #[error("desk record capacity exceeded")]
    Capacity,
}

fn validate_name(name: &str) -> Result<(), DeskError> {
    if name.is_empty()
        || name.chars().take(limits::PEER_NAME_MAX + 1).count() > limits::PEER_NAME_MAX
    {
        return Err(DeskError::BadName);
    }
    Ok(())
}

/// Canonical member signing bytes (PROTOCOL.md 6). Name length counts UTF-8 bytes.
///
/// # Errors
/// Returns [`DeskError::BadName`] for a name outside the wire schema's bound.
pub fn member_signing_input(
    id: MachineId,
    pin: Pin,
    name: &str,
    introduced_by: MachineId,
    added_at: u64,
) -> Result<Vec<u8>, DeskError> {
    validate_name(name)?;
    let name_len = u16::try_from(name.len()).map_err(|_| DeskError::BadName)?;
    let mut input = MEMBER_DOMAIN.to_vec();
    input.extend_from_slice(id.as_bytes());
    input.extend_from_slice(pin.as_bytes());
    input.extend_from_slice(&name_len.to_be_bytes());
    input.extend_from_slice(name.as_bytes());
    input.extend_from_slice(introduced_by.as_bytes());
    input.extend_from_slice(&added_at.to_be_bytes());
    Ok(input)
}

/// Canonical revocation signing bytes (PROTOCOL.md 6).
#[must_use]
pub fn revocation_signing_input(id: MachineId, revoked_by: MachineId, revoked_at: u64) -> Vec<u8> {
    let mut input = REVOCATION_DOMAIN.to_vec();
    input.extend_from_slice(id.as_bytes());
    input.extend_from_slice(revoked_by.as_bytes());
    input.extend_from_slice(&revoked_at.to_be_bytes());
    input
}

/// A structurally checked membership record; trust and signatures are checked by merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub(super) id: MachineId,
    pub(super) pin: Pin,
    pub(super) name: String,
    pub(super) introduced_by: MachineId,
    pub(super) added_at: u64,
    pub(super) signature: [u8; SIGNATURE_LEN],
}

impl Member {
    #[must_use]
    pub fn id(&self) -> MachineId {
        self.id
    }
    #[must_use]
    pub fn pin(&self) -> Pin {
        self.pin
    }
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    #[must_use]
    pub fn introduced_by(&self) -> MachineId {
        self.introduced_by
    }
    #[must_use]
    pub fn added_at(&self) -> u64 {
        self.added_at
    }
    #[must_use]
    pub fn signature(&self) -> &[u8] {
        &self.signature
    }

    pub(super) fn signing_input(&self) -> Vec<u8> {
        member_signing_input(
            self.id,
            self.pin,
            &self.name,
            self.introduced_by,
            self.added_at,
        )
        .expect("Member has a checked name")
    }
}

impl TryFrom<&DeskMember> for Member {
    type Error = DeskError;

    fn try_from(record: &DeskMember) -> Result<Self, Self::Error> {
        validate_name(&record.name)?;
        Ok(Self {
            id: MachineId::try_from(record.id.as_slice())?,
            pin: Pin::try_from(record.pin.as_slice())?,
            name: record.name.clone(),
            introduced_by: MachineId::try_from(record.introduced_by.as_slice())?,
            added_at: record.added_at,
            signature: record
                .signature
                .as_slice()
                .try_into()
                .map_err(|_| DeskError::BadSignature)?,
        })
    }
}

impl From<&Member> for DeskMember {
    fn from(record: &Member) -> Self {
        Self {
            id: record.id.as_bytes().to_vec(),
            pin: record.pin.as_bytes().to_vec(),
            name: record.name.clone(),
            introduced_by: record.introduced_by.as_bytes().to_vec(),
            added_at: record.added_at,
            signature: record.signature.to_vec(),
        }
    }
}

/// A structurally checked revocation record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revocation {
    pub(super) id: MachineId,
    pub(super) revoked_by: MachineId,
    pub(super) revoked_at: u64,
    pub(super) signature: [u8; SIGNATURE_LEN],
}

impl Revocation {
    #[must_use]
    pub fn id(&self) -> MachineId {
        self.id
    }
    #[must_use]
    pub fn revoked_by(&self) -> MachineId {
        self.revoked_by
    }
    #[must_use]
    pub fn revoked_at(&self) -> u64 {
        self.revoked_at
    }
    #[must_use]
    pub fn signature(&self) -> &[u8] {
        &self.signature
    }
}

impl TryFrom<&DeskRevocation> for Revocation {
    type Error = DeskError;

    fn try_from(record: &DeskRevocation) -> Result<Self, Self::Error> {
        Ok(Self {
            id: MachineId::try_from(record.id.as_slice())?,
            revoked_by: MachineId::try_from(record.revoked_by.as_slice())?,
            revoked_at: record.revoked_at,
            signature: record
                .signature
                .as_slice()
                .try_into()
                .map_err(|_| DeskError::BadSignature)?,
        })
    }
}

impl From<&Revocation> for DeskRevocation {
    fn from(record: &Revocation) -> Self {
        Self {
            id: record.id.as_bytes().to_vec(),
            revoked_by: record.revoked_by.as_bytes().to_vec(),
            revoked_at: record.revoked_at,
            signature: record.signature.to_vec(),
        }
    }
}
