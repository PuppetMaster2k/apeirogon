// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn poll_hold_deadline_fires_home_once() {
    // Arrange
    for trigger in ["right_alt", "f13"] {
        let mut matcher = matcher();
        matcher.bindings.trigger_mode = TriggerMode::Hold;
        matcher.bindings.double_tap = DoubleTap::parse(trigger).unwrap();
        let usage = key_usage(trigger).unwrap();
        let now = Instant::now();
        let press = matcher.on_key(usage, true, now);
        let deadline = now + matcher.bindings.hold;
        matcher.on_key(
            usage,
            true,
            deadline.checked_sub(Duration::from_millis(1)).unwrap(),
        );

        // Act
        let before = matcher.poll(deadline.checked_sub(Duration::from_millis(1)).unwrap());
        let due = matcher.poll(deadline);
        let again = matcher.poll(deadline);
        let release = matcher.on_key(usage, false, deadline);

        // Assert
        assert_eq!(before, None);
        assert_eq!(due, Some(Hotkey::Home));
        assert_eq!(again, None);
        assert_eq!(release.action, None);
        assert_eq!(matcher.next_deadline(), None);
        assert!(press.forward);
        assert_eq!(release.forward, trigger == "right_alt");
    }
}

#[test]
fn poll_release_or_another_key_cancels_hold() {
    // Arrange
    for (usage, down) in [
        (0x0007_00e6, false),
        (0x0007_0004, true),
        (0x0007_00e0, true),
        (0x0007_00e2, true),
        (0x000c_0001, true),
    ] {
        let mut matcher = matcher();
        matcher.bindings.trigger_mode = TriggerMode::Hold;
        matcher.bindings.double_tap = DoubleTap::parse("either_alt").unwrap();
        let now = Instant::now();
        matcher.on_key(0x0007_00e6, true, now);
        matcher.on_key(usage, down, now + Duration::from_millis(100));
        if down {
            matcher.on_key(0x0007_00e6, true, now + Duration::from_millis(200));
        }

        // Act
        let action = matcher.poll(now + matcher.bindings.hold);

        // Assert
        assert_eq!(action, None);
        assert_eq!(matcher.next_deadline(), None);
    }
}

#[test]
fn parse_trigger_keys_accepts_only_non_typing_keys() {
    // Arrange
    let accepted = [
        "f1",
        "f12",
        "f13",
        "f24",
        "scroll_lock",
        "pause",
        "menu",
        "left_ctrl",
        "right_alt",
        "either_shift",
        "none",
    ];
    let rejected = [
        "a",
        "1",
        "space",
        "caps_lock",
        "escape",
        "num_lock",
        "enter",
        "tab",
        "backspace",
        "left",
        "f0",
        "f25",
    ];

    // Act
    let valid = accepted.map(DoubleTap::parse);
    let invalid = rejected.map(DoubleTap::parse);

    // Assert
    assert!(valid.iter().all(Result::is_ok));
    assert!(invalid.iter().all(Result::is_err));
}
