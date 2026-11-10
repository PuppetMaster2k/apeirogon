// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn matcher() -> HotkeyMatcher {
    HotkeyMatcher::new(Bindings {
        trigger_mode: TriggerMode::DoubleTap,
        hold: Duration::from_millis(u64::from(HOLD_DEFAULT)),
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

mod hold;
