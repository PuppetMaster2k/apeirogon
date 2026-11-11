// SPDX-License-Identifier: GPL-3.0-or-later

use super::{KEYBOARD_PAGE, modifier_bit};

const LETTER_FIRST: u32 = 0x04;
const LETTER_LAST: u32 = 0x1d;
const DIGIT_FIRST: u32 = 0x1e;
const DIGIT_LAST: u32 = 0x26;
const DIGIT_ZERO: u32 = 0x27;

const KEYS: &[(&str, u32)] = &[
    ("f1", 0x3a),
    ("f2", 0x3b),
    ("f3", 0x3c),
    ("f4", 0x3d),
    ("f5", 0x3e),
    ("f6", 0x3f),
    ("f7", 0x40),
    ("f8", 0x41),
    ("f9", 0x42),
    ("f10", 0x43),
    ("f11", 0x44),
    ("f12", 0x45),
    ("f13", 0x68),
    ("f14", 0x69),
    ("f15", 0x6a),
    ("f16", 0x6b),
    ("f17", 0x6c),
    ("f18", 0x6d),
    ("f19", 0x6e),
    ("f20", 0x6f),
    ("f21", 0x70),
    ("f22", 0x71),
    ("f23", 0x72),
    ("f24", 0x73),
    ("escape", 0x29),
    ("tab", 0x2b),
    ("left", 0x50),
    ("right", 0x4f),
    ("up", 0x52),
    ("down", 0x51),
    ("scroll_lock", 0x47),
    ("pause", 0x48),
    ("insert", 0x49),
    ("home", 0x4a),
    ("page_up", 0x4b),
    ("delete", 0x4c),
    ("end", 0x4d),
    ("page_down", 0x4e),
    ("menu", 0x65),
    ("left_ctrl", 0xe0),
    ("left_shift", 0xe1),
    ("left_alt", 0xe2),
    ("left_meta", 0xe3),
    ("right_ctrl", 0xe4),
    ("right_shift", 0xe5),
    ("right_alt", 0xe6),
    ("right_meta", 0xe7),
];

pub(super) fn key_usage(token: &str) -> Option<u32> {
    let key = match token.as_bytes() {
        [letter @ b'a'..=b'z'] => u32::from(letter - b'a') + LETTER_FIRST,
        [digit @ b'1'..=b'9'] => u32::from(digit - b'1') + DIGIT_FIRST,
        [b'0'] => DIGIT_ZERO,
        _ => KEYS.iter().find(|(name, _)| *name == token)?.1,
    };
    Some(KEYBOARD_PAGE | key)
}

/// Canonical name of a physical key allowed as a return-home trigger.
#[must_use]
pub fn trigger_name(usage: u32) -> Option<&'static str> {
    let key = usage.checked_sub(KEYBOARD_PAGE)?;
    let name = KEYS.iter().find(|(_, value)| *value == key)?.0;
    (name.starts_with('f')
        || matches!(name, "scroll_lock" | "pause" | "menu")
        || modifier_bit(usage).is_some())
    .then_some(name)
}

pub(super) fn key_name(usage: u32) -> Option<String> {
    let key = usage.checked_sub(KEYBOARD_PAGE)?;
    let character = match key {
        LETTER_FIRST..=LETTER_LAST => char::from_u32(u32::from(b'a') + key - LETTER_FIRST),
        DIGIT_FIRST..=DIGIT_LAST => char::from_u32(u32::from(b'1') + key - DIGIT_FIRST),
        DIGIT_ZERO => Some('0'),
        _ => None,
    };
    character.map(|value| value.to_string()).or_else(|| {
        KEYS.iter()
            .find(|(_, value)| *value == key)
            .map(|(name, _)| (*name).to_owned())
    })
}
