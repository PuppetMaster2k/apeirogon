// SPDX-License-Identifier: GPL-3.0-or-later

//! USB HID usages ↔ Windows set-1 scan codes, and consumer usages ↔ virtual keys.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK, VK_MEDIA_STOP, VK_VOLUME_DOWN,
    VK_VOLUME_MUTE, VK_VOLUME_UP,
};

const KBD: u32 = 0x0007_0000;
const CONSUMER: u32 = 0x000C_0000;

/// Each usage and (scan code, extended) pair appears exactly once.
static KEYS: &[(u32, u16, bool)] = &[
    (KBD | 0x04, 0x1E, false), // A
    (KBD | 0x05, 0x30, false), // B
    (KBD | 0x06, 0x2E, false), // C
    (KBD | 0x07, 0x20, false), // D
    (KBD | 0x08, 0x12, false), // E
    (KBD | 0x09, 0x21, false), // F
    (KBD | 0x0A, 0x22, false), // G
    (KBD | 0x0B, 0x23, false), // H
    (KBD | 0x0C, 0x17, false), // I
    (KBD | 0x0D, 0x24, false), // J
    (KBD | 0x0E, 0x25, false), // K
    (KBD | 0x0F, 0x26, false), // L
    (KBD | 0x10, 0x32, false), // M
    (KBD | 0x11, 0x31, false), // N
    (KBD | 0x12, 0x18, false), // O
    (KBD | 0x13, 0x19, false), // P
    (KBD | 0x14, 0x10, false), // Q
    (KBD | 0x15, 0x13, false), // R
    (KBD | 0x16, 0x1F, false), // S
    (KBD | 0x17, 0x14, false), // T
    (KBD | 0x18, 0x16, false), // U
    (KBD | 0x19, 0x2F, false), // V
    (KBD | 0x1A, 0x11, false), // W
    (KBD | 0x1B, 0x2D, false), // X
    (KBD | 0x1C, 0x15, false), // Y
    (KBD | 0x1D, 0x2C, false), // Z
    (KBD | 0x1E, 0x02, false), // 1
    (KBD | 0x1F, 0x03, false), // 2
    (KBD | 0x20, 0x04, false), // 3
    (KBD | 0x21, 0x05, false), // 4
    (KBD | 0x22, 0x06, false), // 5
    (KBD | 0x23, 0x07, false), // 6
    (KBD | 0x24, 0x08, false), // 7
    (KBD | 0x25, 0x09, false), // 8
    (KBD | 0x26, 0x0A, false), // 9
    (KBD | 0x27, 0x0B, false), // 0
    (KBD | 0x28, 0x1C, false), // Enter
    (KBD | 0x29, 0x01, false), // Escape
    (KBD | 0x2A, 0x0E, false), // Backspace
    (KBD | 0x2B, 0x0F, false), // Tab
    (KBD | 0x2C, 0x39, false), // Space
    (KBD | 0x2D, 0x0C, false), // Minus
    (KBD | 0x2E, 0x0D, false), // Equal
    (KBD | 0x2F, 0x1A, false), // Left brace
    (KBD | 0x30, 0x1B, false), // Right brace
    (KBD | 0x31, 0x2B, false), // Backslash
    (KBD | 0x33, 0x27, false), // Semicolon
    (KBD | 0x34, 0x28, false), // Apostrophe
    (KBD | 0x35, 0x29, false), // Grave
    (KBD | 0x36, 0x33, false), // Comma
    (KBD | 0x37, 0x34, false), // Dot
    (KBD | 0x38, 0x35, false), // Slash
    (KBD | 0x39, 0x3A, false), // Caps Lock
    (KBD | 0x3A, 0x3B, false), // F1
    (KBD | 0x3B, 0x3C, false), // F2
    (KBD | 0x3C, 0x3D, false), // F3
    (KBD | 0x3D, 0x3E, false), // F4
    (KBD | 0x3E, 0x3F, false), // F5
    (KBD | 0x3F, 0x40, false), // F6
    (KBD | 0x40, 0x41, false), // F7
    (KBD | 0x41, 0x42, false), // F8
    (KBD | 0x42, 0x43, false), // F9
    (KBD | 0x43, 0x44, false), // F10
    (KBD | 0x44, 0x57, false), // F11
    (KBD | 0x45, 0x58, false), // F12
    (KBD | 0x46, 0x37, true),  // Print Screen
    (KBD | 0x47, 0x46, false), // Scroll Lock
    (KBD | 0x48, 0x45, false), // Pause (E1; injected via VK_PAUSE)
    (KBD | 0x49, 0x52, true),  // Insert
    (KBD | 0x4A, 0x47, true),  // Home
    (KBD | 0x4B, 0x49, true),  // Page Up
    (KBD | 0x4C, 0x53, true),  // Delete
    (KBD | 0x4D, 0x4F, true),  // End
    (KBD | 0x4E, 0x51, true),  // Page Down
    (KBD | 0x4F, 0x4D, true),  // Right
    (KBD | 0x50, 0x4B, true),  // Left
    (KBD | 0x51, 0x50, true),  // Down
    (KBD | 0x52, 0x48, true),  // Up
    (KBD | 0x53, 0x45, true),  // Num Lock
    (KBD | 0x54, 0x35, true),  // Keypad /
    (KBD | 0x55, 0x37, false), // Keypad *
    (KBD | 0x56, 0x4A, false), // Keypad -
    (KBD | 0x57, 0x4E, false), // Keypad +
    (KBD | 0x58, 0x1C, true),  // Keypad Enter
    (KBD | 0x59, 0x4F, false), // Keypad 1
    (KBD | 0x5A, 0x50, false), // Keypad 2
    (KBD | 0x5B, 0x51, false), // Keypad 3
    (KBD | 0x5C, 0x4B, false), // Keypad 4
    (KBD | 0x5D, 0x4C, false), // Keypad 5
    (KBD | 0x5E, 0x4D, false), // Keypad 6
    (KBD | 0x5F, 0x47, false), // Keypad 7
    (KBD | 0x60, 0x48, false), // Keypad 8
    (KBD | 0x61, 0x49, false), // Keypad 9
    (KBD | 0x62, 0x52, false), // Keypad 0
    (KBD | 0x63, 0x53, false), // Keypad .
    (KBD | 0x64, 0x56, false), // Non-US backslash (102nd key)
    (KBD | 0x65, 0x5D, true),  // Application
    (KBD | 0x68, 0x64, false), // F13
    (KBD | 0x69, 0x65, false), // F14
    (KBD | 0x6A, 0x66, false), // F15
    (KBD | 0x6B, 0x67, false), // F16
    (KBD | 0x6C, 0x68, false), // F17
    (KBD | 0x6D, 0x69, false), // F18
    (KBD | 0x6E, 0x6A, false), // F19
    (KBD | 0x6F, 0x6B, false), // F20
    (KBD | 0x70, 0x6C, false), // F21
    (KBD | 0x71, 0x6D, false), // F22
    (KBD | 0x72, 0x6E, false), // F23
    (KBD | 0x73, 0x76, false), // F24
    (KBD | 0x87, 0x73, false), // International1 (Ro)
    (KBD | 0x88, 0x70, false), // International2 (Kana)
    (KBD | 0x89, 0x7D, false), // International3 (Yen)
    (KBD | 0x8A, 0x79, false), // International4 (Convert)
    (KBD | 0x8B, 0x7B, false), // International5 (Nonconvert)
    (KBD | 0xE0, 0x1D, false), // Left Ctrl
    (KBD | 0xE1, 0x2A, false), // Left Shift
    (KBD | 0xE2, 0x38, false), // Left Alt
    (KBD | 0xE3, 0x5B, true),  // Left Meta
    (KBD | 0xE4, 0x1D, true),  // Right Ctrl
    (KBD | 0xE5, 0x36, false), // Right Shift
    (KBD | 0xE6, 0x38, true),  // Right Alt
    (KBD | 0xE7, 0x5C, true),  // Right Meta
];

static MEDIA: &[(u32, u16)] = &[
    (CONSUMER | 0xB5, VK_MEDIA_NEXT_TRACK),
    (CONSUMER | 0xB6, VK_MEDIA_PREV_TRACK),
    (CONSUMER | 0xB7, VK_MEDIA_STOP),
    (CONSUMER | 0xCD, VK_MEDIA_PLAY_PAUSE),
    (CONSUMER | 0xE2, VK_VOLUME_MUTE),
    (CONSUMER | 0xE9, VK_VOLUME_UP),
    (CONSUMER | 0xEA, VK_VOLUME_DOWN),
];

pub(super) fn hid_from_scan(scan: u32, extended: bool) -> Option<u32> {
    KEYS.iter()
        .find(|&&(_, code, ext)| u32::from(code) == scan && ext == extended)
        .map(|&(hid, _, _)| hid)
}

pub(super) fn scan_from_hid(usage: u32) -> Option<(u16, bool)> {
    KEYS.iter()
        .find(|&&(hid, _, _)| hid == usage)
        .map(|&(_, scan, ext)| (scan, ext))
}

pub(super) fn hid_from_media(vk: u32) -> Option<u32> {
    MEDIA
        .iter()
        .find(|&&(_, key)| u32::from(key) == vk)
        .map(|&(hid, _)| hid)
}

pub(super) fn media_from_hid(usage: u32) -> Option<u16> {
    MEDIA
        .iter()
        .find(|&&(hid, _)| hid == usage)
        .map(|&(_, vk)| vk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_from_hid_whole_table_round_trips() {
        // Arrange
        let table = KEYS;

        for &(hid, scan, ext) in table {
            // Act
            let back = hid_from_scan(u32::from(scan), ext).and_then(scan_from_hid);

            // Assert
            assert_eq!(back, Some((scan, ext)));
            assert_eq!(hid_from_scan(u32::from(scan), ext), Some(hid));
        }
    }

    #[test]
    fn scan_from_hid_extended_keys_are_distinct_from_keypad_and_left_modifiers() {
        // Arrange
        let cases = [
            (0xE4, 0x1D),
            (0xE6, 0x38),
            (0x4F, 0x4D),
            (0x50, 0x4B),
            (0x51, 0x50),
            (0x52, 0x48),
            (0x58, 0x1C),
        ];

        for (id, scan) in cases {
            // Act
            let mapped = scan_from_hid(KBD | id);

            // Assert
            assert_eq!(mapped, Some((scan, true)));
            assert_ne!(hid_from_scan(u32::from(scan), false), Some(KBD | id));
        }
    }

    #[test]
    fn media_from_hid_whole_table_round_trips() {
        // Arrange
        let table = MEDIA;

        for &(hid, vk) in table {
            // Act
            let back = hid_from_media(u32::from(vk)).and_then(media_from_hid);

            // Assert
            assert_eq!(back, Some(vk));
            assert_eq!(hid_from_media(u32::from(vk)), Some(hid));
        }
    }

    #[test]
    fn hid_from_scan_unknown_codes_are_dropped() {
        // Arrange
        let codes = [0, 0xFF, u32::MAX];

        // Act
        let mapped = codes.map(|code| hid_from_scan(code, false));

        // Assert
        assert_eq!(mapped, [None; 3]);
        assert_eq!(scan_from_hid(u32::MAX), None);
        assert_eq!(media_from_hid(u32::MAX), None);
    }
}
