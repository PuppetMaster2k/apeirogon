// SPDX-License-Identifier: GPL-3.0-or-later

use super::{Chord, HotkeyError, MODIFIER_COUNT, Mods, keys::key_name, modifier_bit, trigger_name};

const SIDE_MODIFIERS: u32 = MODIFIER_COUNT / 2;
const SIDE_MODIFIER_MASK: u8 = (1 << SIDE_MODIFIERS) - 1;

/// Config name for one physical return-home trigger.
/// # Errors
/// Rejects multiple keys and keys unsuitable as triggers.
pub fn recorded_trigger(usages: &[u32]) -> Result<String, HotkeyError> {
    let [usage] = usages else {
        return Err(HotkeyError("press exactly one trigger key".into()));
    };
    trigger_name(*usage).map(str::to_owned).ok_or_else(|| {
        HotkeyError(
            "that key types text; pick a modifier, F-key, Scroll Lock, Pause or Menu".into(),
        )
    })
}

/// Config chord with generic modifiers and exactly one non-modifier key.
/// # Errors
/// Rejects missing, unknown or multiple non-modifier keys.
pub fn recorded_chord(usages: &[u32]) -> Result<String, HotkeyError> {
    let mut keys = usages
        .iter()
        .copied()
        .filter(|usage| modifier_bit(*usage).is_none());
    let key = keys
        .next()
        .ok_or_else(|| HotkeyError("a chord needs one non-modifier key".into()))?;
    if keys.next().is_some() {
        return Err(HotkeyError(
            "a chord needs exactly one non-modifier key".into(),
        ));
    }
    let name =
        key_name(key).ok_or_else(|| HotkeyError("that key cannot be used in a chord".into()))?;
    let mut parts = modifier_names(usages);
    parts.push(&name);
    let value = parts.join("+");
    Chord::parse(&value)?;
    Ok(value)
}

/// Config names for generic jump modifiers (either physical side).
/// # Errors
/// Rejects an empty selection or any non-modifier key.
pub fn recorded_modifiers(usages: &[u32]) -> Result<String, HotkeyError> {
    if usages.is_empty() || usages.iter().any(|usage| modifier_bit(*usage).is_none()) {
        return Err(HotkeyError(
            "press only modifiers: Ctrl, Alt, Shift or Meta".into(),
        ));
    }
    Ok(modifier_names(usages).join("+"))
}

fn modifier_names(usages: &[u32]) -> Vec<&'static str> {
    let held = usages
        .iter()
        .filter_map(|usage| modifier_bit(*usage))
        .fold(0, |held, bit| held | bit);
    let generic = (held & SIDE_MODIFIER_MASK) | (held >> SIDE_MODIFIERS);
    // Match the usual config spelling while collapsing left and right sides.
    ["ctrl", "alt", "shift", "meta"]
        .into_iter()
        .filter(|name| Mods::token(name).is_some_and(|mods| generic & mods.0 != 0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkey::{Chord, DoubleTap, Mods, keys::key_usage};

    #[test]
    fn recorded_chord_mixed_sides_round_trips() {
        // Arrange
        let usages =
            ["left_ctrl", "right_alt", "left_shift", "home"].map(|name| key_usage(name).unwrap());

        // Act
        let value = recorded_chord(&usages).unwrap();

        // Assert
        assert_eq!(value, "ctrl+alt+shift+home");
        assert_eq!(
            Chord::parse(&value).unwrap(),
            Chord::parse("ctrl+alt+shift+home").unwrap()
        );
    }

    #[test]
    fn recorded_chord_supported_keys_round_trip() {
        // Arrange
        let names = ["a", "z", "0", "1", "9", "f24", "scroll_lock", "page_up"];

        for name in names {
            // Act
            let value = recorded_chord(&[key_usage(name).unwrap()]).unwrap();

            // Assert
            assert_eq!(Chord::parse(&value).unwrap().key, key_usage(name).unwrap());
        }
    }

    #[test]
    fn recorded_modifiers_both_sides_round_trip_without_duplicates() {
        // Arrange
        let usages = [
            "left_ctrl",
            "right_ctrl",
            "right_alt",
            "left_shift",
            "right_meta",
        ]
        .map(|name| key_usage(name).unwrap());

        // Act
        let value = recorded_modifiers(&usages).unwrap();

        // Assert
        assert_eq!(value, "ctrl+alt+shift+meta");
        assert_eq!(
            Mods::parse(&value).unwrap(),
            Mods::parse("ctrl+alt+shift+meta").unwrap()
        );
    }

    #[test]
    fn recorded_trigger_allowed_key_round_trips() {
        // Arrange
        let usage = key_usage("right_alt").unwrap();

        // Act
        let value = recorded_trigger(&[usage]).unwrap();

        // Assert
        assert!(DoubleTap::parse(&value).unwrap().matches(usage));
    }

    #[test]
    fn recorded_value_invalid_combinations_are_rejected() {
        // Arrange
        let a = key_usage("a").unwrap();
        let ctrl = key_usage("left_ctrl").unwrap();
        let cases = [
            (recorded_trigger as fn(&[u32]) -> _, vec![]),
            (recorded_trigger, vec![a]),
            (recorded_trigger, vec![ctrl, a]),
            (recorded_chord, vec![]),
            (recorded_chord, vec![ctrl]),
            (recorded_chord, vec![a, key_usage("b").unwrap()]),
            (recorded_chord, vec![0]),
            (recorded_chord, vec![key_usage("volume_up").unwrap()]),
            (recorded_modifiers, vec![]),
            (recorded_modifiers, vec![ctrl, a]),
        ];

        for (record, usages) in cases {
            // Act
            let result = record(&usages);

            // Assert
            assert!(result.is_err(), "{usages:?}");
        }
    }
}
