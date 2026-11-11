// SPDX-License-Identifier: GPL-3.0-or-later

use super::{KEYBOARD_PAGE, modifier_bit};

const LETTER_FIRST: u32 = 0x04;
const LETTER_LAST: u32 = 0x1d;
const DIGIT_FIRST: u32 = 0x1e;
const DIGIT_LAST: u32 = 0x26;
const DIGIT_ZERO: u32 = 0x27;

const CONSUMER_PAGE: u32 = 0x000c_0000;

const KEYS: &[(&str, u32)] = &[
    ("mute", CONSUMER_PAGE | 0xe2),
    ("volume_up", CONSUMER_PAGE | 0xe9),
    ("volume_down", CONSUMER_PAGE | 0xea),
    ("play_pause", CONSUMER_PAGE | 0xcd),
    ("stop", CONSUMER_PAGE | 0xb7),
    ("next_track", CONSUMER_PAGE | 0xb5),
    ("previous_track", CONSUMER_PAGE | 0xb6),
    ("f1", KEYBOARD_PAGE | 0x3a),
    ("f2", KEYBOARD_PAGE | 0x3b),
    ("f3", KEYBOARD_PAGE | 0x3c),
    ("f4", KEYBOARD_PAGE | 0x3d),
    ("f5", KEYBOARD_PAGE | 0x3e),
    ("f6", KEYBOARD_PAGE | 0x3f),
    ("f7", KEYBOARD_PAGE | 0x40),
    ("f8", KEYBOARD_PAGE | 0x41),
    ("f9", KEYBOARD_PAGE | 0x42),
    ("f10", KEYBOARD_PAGE | 0x43),
    ("f11", KEYBOARD_PAGE | 0x44),
    ("f12", KEYBOARD_PAGE | 0x45),
    ("f13", KEYBOARD_PAGE | 0x68),
    ("f14", KEYBOARD_PAGE | 0x69),
    ("f15", KEYBOARD_PAGE | 0x6a),
    ("f16", KEYBOARD_PAGE | 0x6b),
    ("f17", KEYBOARD_PAGE | 0x6c),
    ("f18", KEYBOARD_PAGE | 0x6d),
    ("f19", KEYBOARD_PAGE | 0x6e),
    ("f20", KEYBOARD_PAGE | 0x6f),
    ("f21", KEYBOARD_PAGE | 0x70),
    ("f22", KEYBOARD_PAGE | 0x71),
    ("f23", KEYBOARD_PAGE | 0x72),
    ("f24", KEYBOARD_PAGE | 0x73),
    ("escape", KEYBOARD_PAGE | 0x29),
    ("tab", KEYBOARD_PAGE | 0x2b),
    ("left", KEYBOARD_PAGE | 0x50),
    ("right", KEYBOARD_PAGE | 0x4f),
    ("up", KEYBOARD_PAGE | 0x52),
    ("down", KEYBOARD_PAGE | 0x51),
    ("scroll_lock", KEYBOARD_PAGE | 0x47),
    ("pause", KEYBOARD_PAGE | 0x48),
    ("insert", KEYBOARD_PAGE | 0x49),
    ("home", KEYBOARD_PAGE | 0x4a),
    ("page_up", KEYBOARD_PAGE | 0x4b),
    ("delete", KEYBOARD_PAGE | 0x4c),
    ("end", KEYBOARD_PAGE | 0x4d),
    ("page_down", KEYBOARD_PAGE | 0x4e),
    ("menu", KEYBOARD_PAGE | 0x65),
    ("left_ctrl", KEYBOARD_PAGE | 0xe0),
    ("left_shift", KEYBOARD_PAGE | 0xe1),
    ("left_alt", KEYBOARD_PAGE | 0xe2),
    ("left_meta", KEYBOARD_PAGE | 0xe3),
    ("right_ctrl", KEYBOARD_PAGE | 0xe4),
    ("right_shift", KEYBOARD_PAGE | 0xe5),
    ("right_alt", KEYBOARD_PAGE | 0xe6),
    ("right_meta", KEYBOARD_PAGE | 0xe7),
];

/// Resolve a physical key name to its HID usage.
#[must_use]
pub fn key_usage(token: &str) -> Option<u32> {
    let key = match token.as_bytes() {
        [letter @ b'a'..=b'z'] => u32::from(letter - b'a') + LETTER_FIRST,
        [digit @ b'1'..=b'9'] => u32::from(digit - b'1') + DIGIT_FIRST,
        [b'0'] => DIGIT_ZERO,
        _ => {
            return KEYS
                .iter()
                .find(|(name, _)| *name == token)
                .map(|(_, usage)| *usage);
        }
    };
    Some(KEYBOARD_PAGE | key)
}

/// Canonical name of a physical key allowed as a return-home trigger.
#[must_use]
pub fn trigger_name(usage: u32) -> Option<&'static str> {
    let name = KEYS.iter().find(|(_, value)| *value == usage)?.0;
    (name.starts_with('f')
        || matches!(name, "scroll_lock" | "pause" | "menu")
        || modifier_bit(usage).is_some())
    .then_some(name)
}

/// Canonical name of a physical key, including media keys.
#[must_use]
pub fn key_name(usage: u32) -> Option<String> {
    let key = usage.wrapping_sub(KEYBOARD_PAGE);
    let character = match key {
        LETTER_FIRST..=LETTER_LAST => char::from_u32(u32::from(b'a') + key - LETTER_FIRST),
        DIGIT_FIRST..=DIGIT_LAST => char::from_u32(u32::from(b'1') + key - DIGIT_FIRST),
        DIGIT_ZERO => Some('0'),
        _ => None,
    };
    character.map(|value| value.to_string()).or_else(|| {
        KEYS.iter()
            .find(|(_, value)| *value == usage)
            .map(|(name, _)| (*name).to_owned())
    })
}
