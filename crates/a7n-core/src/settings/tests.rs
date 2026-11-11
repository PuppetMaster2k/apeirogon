// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn settings(set_at: u64, by: u8) -> Settings {
    Settings {
        set_at,
        by: MachineId::try_from(&[by; MachineId::LEN][..]).unwrap(),
        values: BTreeMap::new(),
    }
}

#[test]
fn try_from_unknown_and_security_keys_drops_everything_outside_allow_list() {
    // Arrange
    let mut wire = settings(1, 1).to_proto();
    wire.values = [
        ("input.speed".into(), "2".into()),
        ("input.fullscreen_hold".into(), "true".into()),
        ("input.hide_idle_cursor".into(), "false".into()),
        (
            "peers.01010101010101010101010101010101.allow_drive".into(),
            "false".into(),
        ),
        ("name".into(), "remote".into()),
        ("sync.settings".into(), "false".into()),
        ("clipboard.policy".into(), "allow".into()),
        (
            "peers.01010101010101010101010101010101.clipboard".into(),
            "both".into(),
        ),
        ("clipboard.deny_apps".into(), "[]".into()),
        ("notifications.files".into(), "false".into()),
        ("unknown".into(), "value".into()),
    ]
    .into();

    // Act
    let filtered = Settings::try_from(&wire).unwrap();

    // Assert
    assert_eq!(filtered.values, [("input.speed".into(), "2".into())].into());
}

#[test]
fn newer_timestamp_and_machine_id_order_requires_strict_increase() {
    // Arrange
    let current = settings(10, 2);
    for (incoming, expected) in [
        (settings(9, 3), false),
        (settings(10, 1), false),
        (settings(10, 2), false),
        (settings(10, 3), true),
        (settings(11, 1), true),
    ] {
        // Act
        let result = newer(Some(&current), &incoming);

        // Assert
        assert_eq!(result, expected);
    }
    assert!(newer(None, &current));
}

#[test]
fn try_from_invalid_machine_id_or_counter_above_max_rejects_message() {
    // Arrange
    let bad_id = v1::SettingsSync {
        by: vec![1; MachineId::LEN - 1],
        ..Default::default()
    };
    let bad_counter = settings(a7n_proto::limits::COUNTER_MAX + 1, 1).to_proto();

    // Act
    let id_result = Settings::try_from(&bad_id);
    let counter_result = Settings::try_from(&bad_counter);

    // Assert
    assert!(matches!(id_result, Err(ProtoError::BadMachineId(_))));
    assert!(matches!(counter_result, Err(ProtoError::CounterTooLarge)));
}
