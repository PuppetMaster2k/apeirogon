// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use a7n_proto::Pin;

fn id(byte: u8) -> MachineId {
    MachineId::try_from(&[byte; MachineId::LEN][..]).unwrap()
}

fn member(byte: u8, added_at: u64) -> DeskMember {
    DeskMember {
        id: id(byte).as_bytes().to_vec(),
        pin: vec![byte; 32],
        name: format!("peer-{byte}"),
        introduced_by: id(1).as_bytes().to_vec(),
        added_at,
        signature: vec![1; records::SIGNATURE_LEN],
    }
}

fn indexed_member(index: usize, added_at: u64) -> DeskMember {
    let mut record = member(3, added_at);
    let bytes = index.to_be_bytes();
    record.id[..bytes.len()].copy_from_slice(&bytes);
    record
}

fn revoke(byte: u8, revoked_at: u64) -> DeskRevocation {
    DeskRevocation {
        id: id(byte).as_bytes().to_vec(),
        revoked_by: id(1).as_bytes().to_vec(),
        revoked_at,
        signature: vec![1; records::SIGNATURE_LEN],
    }
}

fn merge(
    state: &mut DeskState,
    members: Vec<DeskMember>,
    revocations: Vec<DeskRevocation>,
) -> MergeOutcome {
    state.merge(
        &Desk {
            members,
            revocations,
        },
        &|signer| signer == id(1),
        |_, _, _| true,
    )
}

#[test]
fn merge_two_desks_unions_members() {
    // Arrange
    let mut state = DeskState::new(id(9));
    merge(&mut state, vec![member(2, 10)], vec![]);

    // Act
    let outcome = merge(&mut state, vec![member(3, 20)], vec![]);

    // Assert
    assert_eq!(
        state.members().map(Member::id).collect::<Vec<_>>(),
        [id(2), id(3)]
    );
    assert_eq!(
        outcome.admitted.iter().map(Member::id).collect::<Vec<_>>(),
        [id(3)]
    );
    assert_eq!(outcome.revoked, []);
}

#[test]
fn merge_revocation_at_or_after_admission_removes_member() {
    // Arrange
    for revoked_at in [10, 11] {
        let mut state = DeskState::new(id(9));
        merge(&mut state, vec![member(2, 10)], vec![]);

        // Act
        let outcome = merge(&mut state, vec![], vec![revoke(2, revoked_at)]);

        // Assert
        assert_eq!(state.members().count(), 0);
        assert_eq!(outcome.revoked, [id(2)]);
        assert_eq!(state.to_desk().members, []);
        assert_eq!(state.to_desk().revocations.len(), 1);
    }
}

#[test]
fn merge_later_readmission_survives_revocation() {
    // Arrange
    let mut state = DeskState::new(id(9));
    merge(&mut state, vec![member(2, 10)], vec![revoke(2, 20)]);

    // Act
    let outcome = merge(&mut state, vec![member(2, 21)], vec![revoke(2, 20)]);

    // Assert
    assert_eq!(state.members().next().unwrap().added_at(), 21);
    assert_eq!(outcome.admitted.len(), 1);
    assert_eq!(outcome.revoked, []);
    assert_eq!(state.to_desk().members, [member(2, 21)]);
    assert_eq!(state.to_desk().revocations, [revoke(2, 20)]);
}

#[test]
fn merge_member_and_revocation_order_keeps_same_effective_state() {
    // Arrange
    let mut first = DeskState::new(id(9));
    let mut second = DeskState::new(id(9));
    merge(&mut first, vec![member(2, 10), member(2, 30)], vec![]);
    merge(&mut second, vec![], vec![revoke(2, 20)]);

    // Act
    merge(&mut first, vec![], vec![revoke(2, 20)]);
    merge(&mut second, vec![member(2, 30), member(2, 10)], vec![]);

    // Assert
    assert_eq!(first.to_desk(), second.to_desk());
    assert_eq!(first.members().next().unwrap().added_at(), 30);
}

#[test]
fn merge_same_timestamp_conflicts_converges_on_smaller_signature() {
    // Arrange
    let low = Desk {
        members: vec![member(2, 10)],
        revocations: vec![revoke(3, 20)],
    };
    let mut high = low.clone();
    high.members[0].name = "other".into();
    high.members[0].signature.fill(2);
    high.revocations[0].signature.fill(2);
    let mut first = DeskState::new(id(9));
    let mut second = DeskState::new(id(9));
    let combined = |a: &Desk, b: &Desk| Desk {
        members: [a.members.clone(), b.members.clone()].concat(),
        revocations: [a.revocations.clone(), b.revocations.clone()].concat(),
    };

    // Act
    for (state, records) in [(&mut first, [&high, &low]), (&mut second, [&low, &high])] {
        for desk in records {
            merge(state, desk.members.clone(), desk.revocations.clone());
        }
    }
    let restored_first = DeskState::restore(id(9), &combined(&high, &low)).unwrap();
    let restored_second = DeskState::restore(id(9), &combined(&low, &high)).unwrap();

    // Assert
    for state in [first, second, restored_first, restored_second] {
        assert_eq!(state.to_desk(), low);
    }
}

#[test]
fn merge_stale_records_keeps_only_latest_member_and_revocation() {
    // Arrange
    let desk = Desk {
        members: vec![member(2, 10), member(2, 30), member(2, 21)],
        revocations: vec![revoke(2, 20), revoke(2, 15)],
    };
    let mut state = DeskState::restore(id(9), &desk).unwrap();

    // Act
    let outcome = merge(
        &mut state,
        vec![member(2, 20), member(2, 21)],
        vec![revoke(2, 15)],
    );

    // Assert
    assert_eq!(outcome, MergeOutcome::default());
    assert_eq!(state.to_desk().members, [member(2, 30)]);
    assert_eq!(state.to_desk().revocations, [revoke(2, 20)]);
}

#[test]
fn merge_bad_signatures_rejects_both_record_kinds() {
    // Arrange
    let mut state = DeskState::new(id(9));
    let incoming = Desk {
        members: vec![member(2, 10)],
        revocations: vec![revoke(3, 20)],
    };

    // Act
    let outcome = state.merge(&incoming, &|_| true, |_, _, _| false);

    // Assert
    assert_eq!(state.to_desk(), Desk::default());
    assert_eq!(outcome, MergeOutcome::default());
}

#[test]
fn merge_untrusted_signers_rejects_both_record_kinds() {
    // Arrange
    let mut state = DeskState::new(id(9));
    let incoming = Desk {
        members: vec![member(2, 10)],
        revocations: vec![revoke(3, 20)],
    };

    // Act
    let outcome = state.merge(&incoming, &|_| false, |_, _, _| {
        panic!("must check trust first")
    });

    // Assert
    assert_eq!(state.to_desk(), Desk::default());
    assert_eq!(outcome, MergeOutcome::default());
}

#[test]
fn merge_repeated_records_produces_no_new_outcome() {
    // Arrange
    let mut state = DeskState::new(id(9));
    let members = vec![member(2, 10), member(3, 10)];
    let revocations = vec![revoke(2, 20), revoke(9, 20)];
    merge(&mut state, members.clone(), revocations.clone());

    // Act
    let outcome = merge(&mut state, members, revocations);

    // Assert
    assert_eq!(outcome, MergeOutcome::default());
    assert_eq!(state.to_desk().members.len(), 1);
    assert_eq!(state.to_desk().revocations.len(), 2);
}

#[test]
fn merge_self_revocation_without_member_reports_delta() {
    // Arrange
    let mut state = DeskState::new(id(9));

    // Act
    let outcome = merge(&mut state, vec![], vec![revoke(9, 20)]);

    // Assert
    assert_eq!(outcome.revoked, [id(9)]);
    assert!(outcome.self_revoked);
}

#[test]
fn restore_revoked_member_prevents_stale_resurrection() {
    // Arrange
    let mut state = DeskState::new(id(9));
    merge(&mut state, vec![member(2, 10)], vec![revoke(2, 20)]);
    let mut restored = DeskState::restore(id(9), &state.to_desk()).unwrap();

    // Act
    let outcome = merge(&mut restored, vec![member(2, 10)], vec![]);

    // Assert
    assert_eq!(restored.to_desk().members, []);
    assert_eq!(outcome, MergeOutcome::default());
}

#[test]
fn merge_malformed_records_drops_only_invalid_entries() {
    // Arrange
    let valid = member(2, 10);
    let mut members = Vec::new();
    for field in 0..6 {
        let mut bad = valid.clone();
        match field {
            0 => bad.id.pop().map(|_| ()),
            1 => bad.pin.pop().map(|_| ()),
            2 => {
                bad.name.clear();
                None
            }
            3 => {
                bad.name = "é".repeat(limits::PEER_NAME_MAX + 1);
                None
            }
            4 => bad.introduced_by.pop().map(|_| ()),
            _ => bad.signature.pop().map(|_| ()),
        };
        members.push(bad);
    }
    members.push(valid);
    let mut revocations = Vec::new();
    for field in 0..3 {
        let mut bad = revoke(2, 20);
        match field {
            0 => bad.id.pop(),
            1 => bad.revoked_by.pop(),
            _ => bad.signature.pop(),
        };
        revocations.push(bad);
    }
    let mut state = DeskState::new(id(9));

    // Act
    let outcome = merge(&mut state, members, revocations);

    // Assert
    assert_eq!(outcome.admitted.len(), 1);
    assert_eq!(outcome.revoked, []);
    assert_eq!(state.to_desk().members.len(), 1);
    assert_eq!(state.to_desk().revocations, []);
}

#[test]
fn try_from_unicode_name_boundary_counts_code_points() {
    // Arrange
    for (length, accepted) in [
        (limits::PEER_NAME_MAX, true),
        (limits::PEER_NAME_MAX + 1, false),
    ] {
        let mut record = member(2, 10);
        record.name = "🦀".repeat(length);

        // Act
        let result = Member::try_from(&record);

        // Assert
        assert_eq!(result.is_ok(), accepted);
    }
}

#[test]
fn signing_input_known_fields_matches_protocol_byte_layout() {
    // Arrange
    let pin = Pin::try_from(&[2; 32][..]).unwrap();
    let timestamp = 0x0102_0304_0506_0708;
    let mut expected_member = b"a7n desk member v1\0".to_vec();
    expected_member.extend([1; 16]);
    expected_member.extend([2; 32]);
    expected_member.extend([0, 2, 0xc3, 0xa9]);
    expected_member.extend([3; 16]);
    expected_member.extend([1, 2, 3, 4, 5, 6, 7, 8]);
    let mut expected_revocation = b"a7n desk revoke v1\0".to_vec();
    expected_revocation.extend([1; 16]);
    expected_revocation.extend([3; 16]);
    expected_revocation.extend([1, 2, 3, 4, 5, 6, 7, 8]);

    // Act
    let inputs = [
        member_signing_input(id(1), pin, "é", id(3), timestamp).unwrap(),
        revocation_signing_input(id(1), id(3), timestamp),
    ];

    // Assert
    assert_eq!(inputs, [expected_member, expected_revocation]);
}

#[test]
fn merge_record_capacity_bounds_machines_without_evicting_tombstones() {
    // Arrange
    let mut state = DeskState::new(id(9));
    merge(&mut state, vec![], vec![revoke(2, 20)]);
    let members = (0..DESK_RECORDS_MAX)
        .map(|index| indexed_member(index, 10))
        .collect();

    // Act
    let outcome = merge(&mut state, members, vec![]);

    // Assert
    assert!(outcome.capacity_reached);
    let desk = state.to_desk();
    assert_eq!(
        desk.members.len() + desk.revocations.len(),
        DESK_RECORDS_MAX
    );
    assert_eq!(desk.revocations, [revoke(2, 20)]);
    assert!(DeskState::restore(id(9), &desk).is_ok());
}

#[test]
fn to_desk_maximum_records_fits_control_frame() {
    // Arrange
    let members = (0..DESK_RECORDS_MAX)
        .map(|at| {
            let mut record = indexed_member(at, u64::MAX);
            record.name = "🦀".repeat(limits::PEER_NAME_MAX);
            record
        })
        .collect();
    let state = DeskState::restore(
        id(9),
        &Desk {
            members,
            revocations: vec![],
        },
    )
    .unwrap();
    let envelope = a7n_proto::v1::Envelope {
        body: Some(a7n_proto::v1::envelope::Body::Desk(state.to_desk())),
    };
    let mut bytes = Vec::new();

    // Act
    a7n_proto::encode_frame(&envelope, &mut bytes);

    // Assert
    assert!(bytes.len() <= limits::CONTROL_FRAME_MAX);
}
