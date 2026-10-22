// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure signed desk membership and revocation merge (PROTOCOL.md 6).

use std::collections::{BTreeMap, BTreeSet};

use a7n_proto::{
    MachineId, limits,
    v1::{Desk, DeskMember, DeskRevocation},
};

mod records;
pub use records::{DeskError, Member, Revocation, member_signing_input, revocation_signing_input};

#[cfg(test)]
mod tests;

// Each length-delimited field uses at most a tag and two length bytes. Reserve
// enough for the enclosing Desk record and Envelope as well, so to_desk fits.
const LENGTH_DELIMITED_OVERHEAD: usize = 3;
const UINT64_ENCODED_MAX: usize = 11;
const UTF8_CODE_POINT_MAX: usize = 4;
const ENVELOPE_OVERHEAD: usize = 16;
const SHA256_DIGEST_LEN: usize = 32;
const MEMBER_ENCODED_MAX: usize = 2 * MachineId::LEN
    + SHA256_DIGEST_LEN
    + limits::PEER_NAME_MAX * UTF8_CODE_POINT_MAX
    + records::SIGNATURE_LEN
    + 6 * LENGTH_DELIMITED_OVERHEAD
    + UINT64_ENCODED_MAX;

/// Hard cap on current members plus latest tombstones; worst-case records fit one control frame.
pub const DESK_RECORDS_MAX: usize =
    (limits::CONTROL_FRAME_MAX - ENVELOPE_OVERHEAD) / MEMBER_ENCODED_MAX;

/// Effective changes only. Apply revocations before admissions to the pin store.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct MergeOutcome {
    pub admitted: Vec<Member>,
    pub revoked: Vec<MachineId>,
    pub self_revoked: bool,
    /// New record slots exceeded the hard cap; existing latest tombstones remain.
    pub capacity_reached: bool,
}

/// Latest member and revocation per machine, retaining tombstones across restarts.
/// Members at or before their latest revocation are never retained.
#[derive(Debug, Clone)]
pub struct DeskState {
    own_id: MachineId,
    members: BTreeMap<MachineId, Member>,
    revocations: BTreeMap<MachineId, Revocation>,
}

impl DeskState {
    #[must_use]
    pub fn new(own_id: MachineId) -> Self {
        Self {
            own_id,
            members: BTreeMap::new(),
            revocations: BTreeMap::new(),
        }
    }

    /// Restore already authenticated local state. Never use this on a peer's `Desk`.
    /// Compact superseded records and resolve timestamp ties by smaller signature.
    ///
    /// # Errors
    /// Rejects malformed records and state exceeding [`DESK_RECORDS_MAX`].
    pub fn restore(own_id: MachineId, desk: &Desk) -> Result<Self, DeskError> {
        if desk.members.len().saturating_add(desk.revocations.len()) > DESK_RECORDS_MAX {
            return Err(DeskError::Capacity);
        }
        let mut state = Self::new(own_id);
        for record in &desk.revocations {
            if state.store_revocation(Revocation::try_from(record)?) {
                return Err(DeskError::Capacity);
            }
        }
        for record in &desk.members {
            if state.store_member(Member::try_from(record)?) {
                return Err(DeskError::Capacity);
            }
        }
        Ok(state)
    }

    /// Merge records authenticated against the caller's existing trust snapshot.
    /// Malformed, untrusted and invalidly signed records are ignored independently.
    /// Same-timestamp ties keep the lexicographically smaller signature.
    /// At capacity, replacements and compaction remain possible; reject new slots.
    pub fn merge(
        &mut self,
        incoming: &Desk,
        trusted: &impl Fn(MachineId) -> bool,
        verify: impl Fn(MachineId, &[u8], &[u8]) -> bool,
    ) -> MergeOutcome {
        let before: BTreeMap<_, _> = self
            .members()
            .map(|member| (member.id, member.clone()))
            .collect();
        let before_revoked = self.revoked_ids();
        let mut outcome = MergeOutcome::default();
        // Process tombstones first so admissions cannot consume the remaining slots.
        for record in &incoming.revocations {
            outcome.capacity_reached |= self.merge_revocation(record, trusted, &verify);
        }
        for record in &incoming.members {
            outcome.capacity_reached |= self.merge_member(record, trusted, &verify);
        }
        outcome.admitted = self
            .members()
            .filter(|member| before.get(&member.id) != Some(*member))
            .cloned()
            .collect();
        outcome.revoked = self
            .revoked_ids()
            .difference(&before_revoked)
            .copied()
            .collect();
        outcome.self_revoked = outcome.revoked.contains(&self.own_id);
        outcome
    }

    fn merge_revocation(
        &mut self,
        record: &DeskRevocation,
        trusted: &impl Fn(MachineId) -> bool,
        verify: &impl Fn(MachineId, &[u8], &[u8]) -> bool,
    ) -> bool {
        let Ok(revocation) = Revocation::try_from(record) else {
            return false;
        };
        if !trusted(revocation.revoked_by) {
            return false;
        }
        let input =
            revocation_signing_input(revocation.id, revocation.revoked_by, revocation.revoked_at);
        if !verify(revocation.revoked_by, &input, &revocation.signature) {
            return false;
        }
        self.store_revocation(revocation)
    }

    fn merge_member(
        &mut self,
        record: &DeskMember,
        trusted: &impl Fn(MachineId) -> bool,
        verify: &impl Fn(MachineId, &[u8], &[u8]) -> bool,
    ) -> bool {
        let Ok(member) = Member::try_from(record) else {
            return false;
        };
        if !trusted(member.introduced_by) {
            return false;
        }
        if !verify(
            member.introduced_by,
            &member.signing_input(),
            &member.signature,
        ) {
            return false;
        }
        self.store_member(member)
    }

    fn store_revocation(&mut self, revocation: Revocation) -> bool {
        let id = revocation.id;
        if self.revocations.get(&id).is_some_and(|current| {
            current.revoked_at > revocation.revoked_at
                || (current.revoked_at == revocation.revoked_at
                    && current.signature <= revocation.signature)
        }) {
            return false;
        }
        let overrides_member = self
            .members
            .get(&id)
            .is_some_and(|member| member.added_at <= revocation.revoked_at);
        if !self.revocations.contains_key(&id) && !overrides_member && self.at_capacity() {
            return true;
        }
        if overrides_member {
            self.members.remove(&id);
        }
        self.revocations.insert(id, revocation);
        false
    }

    fn store_member(&mut self, member: Member) -> bool {
        let id = member.id;
        if self
            .revocations
            .get(&id)
            .is_some_and(|revocation| member.added_at <= revocation.revoked_at)
            || self.members.get(&id).is_some_and(|current| {
                current.added_at > member.added_at
                    || (current.added_at == member.added_at
                        && current.signature <= member.signature)
            })
        {
            return false;
        }
        if !self.members.contains_key(&id) && self.at_capacity() {
            return true;
        }
        self.members.insert(id, member);
        false
    }

    fn at_capacity(&self) -> bool {
        self.members.len() + self.revocations.len() >= DESK_RECORDS_MAX
    }

    fn revoked_ids(&self) -> BTreeSet<MachineId> {
        self.revocations
            .keys()
            .copied()
            .filter(|id| !self.members.contains_key(id))
            .collect()
    }

    /// Current effective members, in machine id order.
    pub fn members(&self) -> impl Iterator<Item = &Member> {
        self.members.values()
    }

    /// Compacted members and latest tombstones for persistence and handshake exchange.
    #[must_use]
    pub fn to_desk(&self) -> Desk {
        Desk {
            members: self.members.values().map(DeskMember::from).collect(),
            revocations: self
                .revocations
                .values()
                .map(DeskRevocation::from)
                .collect(),
        }
    }
}
