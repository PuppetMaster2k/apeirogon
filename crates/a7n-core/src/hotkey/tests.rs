// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn matcher() -> HotkeyMatcher {
    HotkeyMatcher::new(Bindings {
        double_tap: DoubleTap::parse("right_alt").unwrap(),
        home: vec![
            Chord::parse("scroll_lock").unwrap(),
            Chord::parse("ctrl+alt+shift+home").unwrap(),
        ],
        lock: Chord::parse("ctrl+alt+shift+l").unwrap(),
        jump_mods: Mods::parse("ctrl+alt+shift").unwrap(),
    })
}

#[test]
fn parse_key_table_accepts_known_names_and_rejects_bad_tokens() {
    // Arrange
    let names = [
        ("A", 0x04),
        ("z", 0x1d),
        ("0", 0x27),
        ("1", 0x1e),
        ("9", 0x26),
        ("f1", 0x3a),
        ("F12", 0x45),
        ("home", 0x4a),
        ("end", 0x4d),
        ("insert", 0x49),
        ("delete", 0x4c),
        ("page_up", 0x4b),
        ("page_down", 0x4e),
        ("scroll_lock", 0x47),
        ("pause", 0x48),
        ("escape", 0x29),
    ];
    for (name, usage) in names {
        // Act
        let chord = Chord::parse(&format!("CTRL+alt+Shift+{name}")).unwrap();

        // Assert
        assert_eq!(chord.key, KEYBOARD_PAGE | usage);
        assert_eq!(chord.mods, Mods::parse("ctrl+alt+shift").unwrap());
    }
    for (text, bad) in [
        ("foo", "foo"),
        ("a+b", "b"),
        ("", ""),
        ("ctrl", "ctrl"),
        ("ctrl+ctrl+a", "ctrl"),
        ("a+", ""),
    ] {
        // Act
        let error = Chord::parse(text).unwrap_err();

        // Assert
        assert!(error.to_string().contains(bad));
    }
}

#[test]
fn on_key_chords_consume_down_repeat_and_matching_up() {
    // Arrange
    let now = Instant::now();
    for (key, mods, action) in [
        (0x47, &[][..], Hotkey::Home),
        (0x4a, &[0xe0, 0xe2, 0xe1][..], Hotkey::Home),
        (0x0f, &[0xe4, 0xe6, 0xe5][..], Hotkey::Lock),
        (0x26, &[0xe0, 0xe2, 0xe1][..], Hotkey::Jump(9)),
    ] {
        let mut matcher = matcher();
        for modifier in mods {
            assert!(matcher.on_key(KEYBOARD_PAGE | modifier, true, now).forward);
        }

        // Act
        let down = matcher.on_key(KEYBOARD_PAGE | key, true, now);
        let repeat = matcher.on_key(KEYBOARD_PAGE | key, true, now);
        for modifier in mods {
            matcher.on_key(KEYBOARD_PAGE | modifier, false, now);
        }
        let up = matcher.on_key(KEYBOARD_PAGE | key, false, now);

        // Assert
        assert_eq!(
            down,
            Verdict {
                forward: false,
                action: Some(action)
            }
        );
        assert_eq!(
            repeat,
            Verdict {
                forward: false,
                action: None
            }
        );
        assert_eq!(
            up,
            Verdict {
                forward: false,
                action: None
            }
        );
    }
}

#[test]
fn on_key_extra_modifier_forwards_chord() {
    // Arrange
    let mut matcher = matcher();
    let now = Instant::now();
    for modifier in [0xe0, 0xe1, 0xe2, 0xe3] {
        matcher.on_key(KEYBOARD_PAGE | modifier, true, now);
    }

    // Act
    let verdict = matcher.on_key(KEYBOARD_PAGE | 0x4a, true, now);

    // Assert
    assert_eq!(
        verdict,
        Verdict {
            forward: true,
            action: None
        }
    );
}

#[test]
fn on_key_double_tap_timing_and_interruption_control_home() {
    // Arrange
    for (delay, interrupt, expected) in [
        (399, None, Some(Hotkey::Home)),
        (400, None, Some(Hotkey::Home)),
        (401, None, None),
        (200, Some(0x04), None),
        (200, Some(0xe0), None),
        (200, Some(0x000c_0001), None),
    ] {
        let mut matcher = matcher();
        let now = Instant::now();
        let alt = KEYBOARD_PAGE | 0xe6;
        let first = matcher.on_key(alt, true, now);
        let release = matcher.on_key(alt, false, now);
        if let Some(key) = interrupt {
            matcher.on_key(if key < 256 { KEYBOARD_PAGE | key } else { key }, true, now);
        }

        // Act
        let second = matcher.on_key(alt, true, now + Duration::from_millis(delay));
        let second_release = matcher.on_key(alt, false, now + Duration::from_millis(delay));

        // Assert
        assert!(first.forward && release.forward && second.forward && second_release.forward);
        assert_eq!(first.action, None);
        assert_eq!(release.action, None);
        assert_eq!(second.action, expected);
        assert_eq!(second_release.action, None);
    }
}

#[test]
fn on_key_expired_second_tap_starts_new_window() {
    // Arrange
    let mut matcher = matcher();
    let now = Instant::now();
    let alt = KEYBOARD_PAGE | 0xe6;
    for delay in [0, 401] {
        matcher.on_key(alt, true, now + Duration::from_millis(delay));
        matcher.on_key(alt, false, now + Duration::from_millis(delay));
    }

    // Act
    let verdict = matcher.on_key(alt, true, now + Duration::from_millis(500));

    // Assert
    assert_eq!(verdict.action, Some(Hotkey::Home));
}

#[test]
fn on_key_modifier_repeat_requires_release_before_second_tap() {
    // Arrange
    let mut matcher = matcher();
    let now = Instant::now();
    matcher.on_key(KEYBOARD_PAGE | 0xe6, true, now);

    // Act
    let repeat = matcher.on_key(KEYBOARD_PAGE | 0xe6, true, now + Duration::from_millis(100));

    // Assert
    assert_eq!(
        repeat,
        Verdict {
            forward: true,
            action: None
        }
    );
}

#[test]
fn on_key_double_tap_sides_match_configured_modifier() {
    // Arrange
    for side in ["left", "right", "either"] {
        for (name, offset) in [("ctrl", 0), ("shift", 1), ("alt", 2), ("meta", 3)] {
            for right in [false, true] {
                let mut matcher = matcher();
                matcher.bindings.double_tap = DoubleTap::parse(&format!("{side}_{name}")).unwrap();
                let usage = KEYBOARD_PAGE | (MODIFIER_START + offset + if right { 4 } else { 0 });
                let now = Instant::now();
                matcher.on_key(usage, true, now);
                matcher.on_key(usage, false, now);

                // Act
                let verdict = matcher.on_key(usage, true, now + Duration::from_millis(100));

                // Assert
                assert_eq!(
                    verdict.action,
                    (side == "either" || (side == "right") == right).then_some(Hotkey::Home)
                );
            }
        }
    }
}
