// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn entry(identity: &Identity, name: &str) -> Entry {
    Entry {
        machine_id: identity.machine_id,
        pin: identity.pin,
        name: name.into(),
        spki_der: Some(identity::spki_of(&identity.cert_der).unwrap()),
        hw_addr: None,
        screen_mm: None,
    }
}

#[test]
fn merge_desk_containing_own_member_stores_only_peer_pins() {
    // Arrange
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let c_dir = tempfile::tempdir().unwrap();
    let a = identity::load_or_create(a_dir.path()).unwrap();
    let b = identity::load_or_create(b_dir.path()).unwrap();
    let c = identity::load_or_create(c_dir.path()).unwrap();
    let mut a_pins = PinStore::load(&a_dir.path().join("pins.toml")).unwrap();
    let mut b_pins = PinStore::load(&b_dir.path().join("pins.toml")).unwrap();
    b_pins.add(entry(&a, "a"));
    let mut a_desk =
        DeskStore::load(&a_dir.path().join("desk.pb"), a.machine_id, &mut a_pins).unwrap();
    let path = b_dir.path().join("desk.pb");
    let mut b_desk = DeskStore::load(&path, b.machine_id, &mut b_pins).unwrap();
    a_desk.admit(&a, &mut a_pins, &entry(&b, "b"), 1).unwrap();
    a_desk.admit(&a, &mut a_pins, &entry(&c, "c"), 2).unwrap();

    // Act
    let outcome = b_desk.merge(&b, &mut b_pins, &a_desk.to_desk()).unwrap();

    // Assert
    assert_eq!(outcome.admitted.len(), 2);
    assert!(b_pins.get(&b.machine_id).is_none());
    assert_eq!(b_pins.get(&a.machine_id), Some(&entry(&a, "a")));
    assert_eq!(b_pins.get(&c.machine_id).unwrap().pin, c.pin);
}

#[test]
fn admit_own_machine_rejects_without_storing_pin_or_desk() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let own = identity::load_or_create(dir.path()).unwrap();
    let mut pins = PinStore::load(&dir.path().join("pins.toml")).unwrap();
    let path = dir.path().join("desk.pb");
    let mut desk = DeskStore::load(&path, own.machine_id, &mut pins).unwrap();

    // Act
    let result = desk.admit(&own, &mut pins, &entry(&own, "self"), 1);

    // Assert
    assert!(matches!(result, Err(DeskStoreError::Rejected)));
    assert!(pins.get(&own.machine_id).is_none());
    assert!(!path.exists());
}

#[test]
fn apply_own_and_peer_revocations_removes_only_peer_pin() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let peer_dir = tempfile::tempdir().unwrap();
    let own = identity::load_or_create(dir.path()).unwrap();
    let peer = identity::load_or_create(peer_dir.path()).unwrap();
    let mut pins = PinStore::load(&dir.path().join("pins.toml")).unwrap();
    pins.add(entry(&own, "self"));
    pins.add(entry(&peer, "peer"));
    let outcome = MergeOutcome {
        revoked: vec![own.machine_id, peer.machine_id],
        self_revoked: true,
        ..MergeOutcome::default()
    };

    // Act
    DeskStore::apply(&mut pins, &outcome, own.machine_id).unwrap();

    // Assert
    assert_eq!(pins.get(&own.machine_id), Some(&entry(&own, "self")));
    assert!(pins.get(&peer.machine_id).is_none());
}

#[test]
fn merge_admission_then_revocation_survives_reload_and_stale_desk() {
    // Arrange
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let c_dir = tempfile::tempdir().unwrap();
    let a = identity::load_or_create(a_dir.path()).unwrap();
    let b = identity::load_or_create(b_dir.path()).unwrap();
    let c = identity::load_or_create(c_dir.path()).unwrap();
    let mut a_pins = PinStore::load(&a_dir.path().join("pins.toml")).unwrap();
    let mut c_pins = PinStore::load(&c_dir.path().join("pins.toml")).unwrap();
    c_pins.add(entry(&a, "a"));
    c_pins.save().unwrap();
    let mut a_desk =
        DeskStore::load(&a_dir.path().join("desk.pb"), a.machine_id, &mut a_pins).unwrap();
    let path = c_dir.path().join("desk.pb");
    let mut c_desk = DeskStore::load(&path, c.machine_id, &mut c_pins).unwrap();

    // Act
    a_desk.admit(&a, &mut a_pins, &entry(&b, "b"), 1).unwrap();
    let old_desk = a_desk.to_desk();
    let admitted = c_desk.merge(&c, &mut c_pins, &old_desk).unwrap();
    let accepted_b = c_pins.get(&b.machine_id).cloned();
    let stale_pins = std::fs::read(c_dir.path().join("pins.toml")).unwrap();
    a_desk.revoke(&a, &mut a_pins, b.machine_id, 2).unwrap();
    let revoked = c_desk.merge(&c, &mut c_pins, &a_desk.to_desk()).unwrap();
    // Simulate a crash between the durable desk write and the pin write.
    std::fs::write(c_dir.path().join("pins.toml"), stale_pins).unwrap();
    let mut c_pins = PinStore::load(&c_dir.path().join("pins.toml")).unwrap();
    let mut reloaded = DeskStore::load(&path, c.machine_id, &mut c_pins).unwrap();
    let stale = reloaded.merge(&c, &mut c_pins, &old_desk).unwrap();

    // Assert
    assert_eq!(admitted.admitted.len(), 1);
    assert_eq!(accepted_b.unwrap().pin, b.pin);
    assert_eq!(revoked.revoked, vec![b.machine_id]);
    assert!(a_pins.get(&b.machine_id).is_none());
    assert!(c_pins.get(&b.machine_id).is_none());
    assert_eq!(stale.admitted.len(), 0);
    assert_eq!(reloaded.to_desk(), a_desk.to_desk());
}

#[test]
fn merge_unknown_signer_spki_drops_record_until_pinned_certificate_arrives() {
    // Arrange
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let c_dir = tempfile::tempdir().unwrap();
    let a = identity::load_or_create(a_dir.path()).unwrap();
    let b = identity::load_or_create(b_dir.path()).unwrap();
    let c = identity::load_or_create(c_dir.path()).unwrap();
    let mut a_pins = PinStore::load(&a_dir.path().join("pins.toml")).unwrap();
    let mut c_pins = PinStore::load(&c_dir.path().join("pins.toml")).unwrap();
    let mut unknown_key = entry(&a, "a");
    unknown_key.spki_der = None;
    c_pins.add(unknown_key);
    let mut a_desk =
        DeskStore::load(&a_dir.path().join("desk.pb"), a.machine_id, &mut a_pins).unwrap();
    let mut c_desk =
        DeskStore::load(&c_dir.path().join("desk.pb"), c.machine_id, &mut c_pins).unwrap();
    a_desk.admit(&a, &mut a_pins, &entry(&b, "b"), 1).unwrap();

    // Act
    let unknown = c_desk.merge(&c, &mut c_pins, &a_desk.to_desk()).unwrap();
    c_pins.record_certificate(&a.cert_der).unwrap();
    let known = c_desk.merge(&c, &mut c_pins, &a_desk.to_desk()).unwrap();

    // Assert
    assert_eq!(unknown.admitted.len(), 0);
    assert_eq!(known.admitted.len(), 1);
    assert_eq!(c_pins.get(&b.machine_id).unwrap().pin, b.pin);
}

#[test]
fn admit_desk_write_failure_leaves_pins_and_state_unchanged() {
    // Arrange
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let a = identity::load_or_create(a_dir.path()).unwrap();
    let b = identity::load_or_create(b_dir.path()).unwrap();
    let mut pins = PinStore::load(&a_dir.path().join("pins.toml")).unwrap();
    let mut desk = DeskStore::load(
        &a_dir.path().join("missing/desk.pb"),
        a.machine_id,
        &mut pins,
    )
    .unwrap();

    // Act
    let result = desk.admit(&a, &mut pins, &entry(&b, "b"), 1);

    // Assert
    assert!(matches!(result, Err(DeskStoreError::Io(_))));
    assert!(pins.get(&b.machine_id).is_none());
    assert_eq!(desk.to_desk().members.len(), 0);
}

#[test]
fn admit_repaired_member_keeps_certificate_key_after_reload() {
    // Arrange
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let a = identity::load_or_create(a_dir.path()).unwrap();
    let b = identity::load_or_create(b_dir.path()).unwrap();
    let mut pins = PinStore::load(&a_dir.path().join("pins.toml")).unwrap();
    let path = a_dir.path().join("desk.pb");
    let mut desk = DeskStore::load(&path, a.machine_id, &mut pins).unwrap();
    desk.admit(&a, &mut pins, &entry(&b, "b"), 1).unwrap();
    desk.revoke(&a, &mut pins, b.machine_id, 2).unwrap();

    // Act
    desk.admit(&a, &mut pins, &entry(&b, "b"), 3).unwrap();
    let mut pins = PinStore::load(&a_dir.path().join("pins.toml")).unwrap();
    let reloaded = DeskStore::load(&path, a.machine_id, &mut pins).unwrap();

    // Assert
    assert_eq!(pins.get(&b.machine_id), Some(&entry(&b, "b")));
    assert_eq!(reloaded.to_desk(), desk.to_desk());
}

#[test]
fn revoke_older_time_rejects_unstored_record() {
    // Arrange
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let a = identity::load_or_create(a_dir.path()).unwrap();
    let b = identity::load_or_create(b_dir.path()).unwrap();
    let mut pins = PinStore::load(&a_dir.path().join("pins.toml")).unwrap();
    let mut desk = DeskStore::load(&a_dir.path().join("desk.pb"), a.machine_id, &mut pins).unwrap();
    desk.admit(&a, &mut pins, &entry(&b, "b"), 1).unwrap();
    desk.revoke(&a, &mut pins, b.machine_id, 3).unwrap();
    let persisted = desk.to_desk();

    // Act
    let result = desk.revoke(&a, &mut pins, b.machine_id, 2);

    // Assert
    assert!(matches!(result, Err(DeskStoreError::Rejected)));
    assert_eq!(desk.to_desk(), persisted);
    assert!(pins.get(&b.machine_id).is_none());
}
