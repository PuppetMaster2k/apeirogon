// SPDX-License-Identifier: GPL-3.0-or-later

//! Linux evdev key codes <-> USB HID usages `(page << 16) | id`.
//!
//! Follows the kernel's `hid-input.c` mapping and the USB HID Usage Tables
//! (keyboard page 0x07, consumer page 0x0C). Shared by the X11 backend (which
//! adds its keycode offset) and the Wayland backend (libei sends evdev codes).

const KBD: u32 = 0x0007_0000;
const CONSUMER: u32 = 0x000C_0000;

/// `(evdev code, HID usage)`. Each side appears at most once.
static KEYS: &[(u16, u32)] = &[
    (30, KBD | 0x04),        // A
    (48, KBD | 0x05),        // B
    (46, KBD | 0x06),        // C
    (32, KBD | 0x07),        // D
    (18, KBD | 0x08),        // E
    (33, KBD | 0x09),        // F
    (34, KBD | 0x0A),        // G
    (35, KBD | 0x0B),        // H
    (23, KBD | 0x0C),        // I
    (36, KBD | 0x0D),        // J
    (37, KBD | 0x0E),        // K
    (38, KBD | 0x0F),        // L
    (50, KBD | 0x10),        // M
    (49, KBD | 0x11),        // N
    (24, KBD | 0x12),        // O
    (25, KBD | 0x13),        // P
    (16, KBD | 0x14),        // Q
    (19, KBD | 0x15),        // R
    (31, KBD | 0x16),        // S
    (20, KBD | 0x17),        // T
    (22, KBD | 0x18),        // U
    (47, KBD | 0x19),        // V
    (17, KBD | 0x1A),        // W
    (45, KBD | 0x1B),        // X
    (21, KBD | 0x1C),        // Y
    (44, KBD | 0x1D),        // Z
    (2, KBD | 0x1E),         // 1
    (3, KBD | 0x1F),         // 2
    (4, KBD | 0x20),         // 3
    (5, KBD | 0x21),         // 4
    (6, KBD | 0x22),         // 5
    (7, KBD | 0x23),         // 6
    (8, KBD | 0x24),         // 7
    (9, KBD | 0x25),         // 8
    (10, KBD | 0x26),        // 9
    (11, KBD | 0x27),        // 0
    (28, KBD | 0x28),        // Enter
    (1, KBD | 0x29),         // Escape
    (14, KBD | 0x2A),        // Backspace
    (15, KBD | 0x2B),        // Tab
    (57, KBD | 0x2C),        // Space
    (12, KBD | 0x2D),        // Minus
    (13, KBD | 0x2E),        // Equal
    (26, KBD | 0x2F),        // Left brace
    (27, KBD | 0x30),        // Right brace
    (43, KBD | 0x31),        // Backslash
    (39, KBD | 0x33),        // Semicolon
    (40, KBD | 0x34),        // Apostrophe
    (41, KBD | 0x35),        // Grave
    (51, KBD | 0x36),        // Comma
    (52, KBD | 0x37),        // Dot
    (53, KBD | 0x38),        // Slash
    (58, KBD | 0x39),        // Caps Lock
    (59, KBD | 0x3A),        // F1
    (60, KBD | 0x3B),        // F2
    (61, KBD | 0x3C),        // F3
    (62, KBD | 0x3D),        // F4
    (63, KBD | 0x3E),        // F5
    (64, KBD | 0x3F),        // F6
    (65, KBD | 0x40),        // F7
    (66, KBD | 0x41),        // F8
    (67, KBD | 0x42),        // F9
    (68, KBD | 0x43),        // F10
    (87, KBD | 0x44),        // F11
    (88, KBD | 0x45),        // F12
    (99, KBD | 0x46),        // Print Screen (SysRq)
    (70, KBD | 0x47),        // Scroll Lock
    (119, KBD | 0x48),       // Pause
    (110, KBD | 0x49),       // Insert
    (102, KBD | 0x4A),       // Home
    (104, KBD | 0x4B),       // Page Up
    (111, KBD | 0x4C),       // Delete
    (107, KBD | 0x4D),       // End
    (109, KBD | 0x4E),       // Page Down
    (106, KBD | 0x4F),       // Right
    (105, KBD | 0x50),       // Left
    (108, KBD | 0x51),       // Down
    (103, KBD | 0x52),       // Up
    (69, KBD | 0x53),        // Num Lock
    (98, KBD | 0x54),        // Keypad /
    (55, KBD | 0x55),        // Keypad *
    (74, KBD | 0x56),        // Keypad -
    (78, KBD | 0x57),        // Keypad +
    (96, KBD | 0x58),        // Keypad Enter
    (79, KBD | 0x59),        // Keypad 1
    (80, KBD | 0x5A),        // Keypad 2
    (81, KBD | 0x5B),        // Keypad 3
    (75, KBD | 0x5C),        // Keypad 4
    (76, KBD | 0x5D),        // Keypad 5
    (77, KBD | 0x5E),        // Keypad 6
    (71, KBD | 0x5F),        // Keypad 7
    (72, KBD | 0x60),        // Keypad 8
    (73, KBD | 0x61),        // Keypad 9
    (82, KBD | 0x62),        // Keypad 0
    (83, KBD | 0x63),        // Keypad .
    (86, KBD | 0x64),        // Non-US backslash (102nd key)
    (127, KBD | 0x65),       // Application (PC menu key, evdev KEY_COMPOSE)
    (117, KBD | 0x67),       // Keypad =
    (183, KBD | 0x68),       // F13
    (184, KBD | 0x69),       // F14
    (185, KBD | 0x6A),       // F15
    (186, KBD | 0x6B),       // F16
    (187, KBD | 0x6C),       // F17
    (188, KBD | 0x6D),       // F18
    (189, KBD | 0x6E),       // F19
    (190, KBD | 0x6F),       // F20
    (191, KBD | 0x70),       // F21
    (192, KBD | 0x71),       // F22
    (193, KBD | 0x72),       // F23
    (194, KBD | 0x73),       // F24
    (139, KBD | 0x76),       // Menu
    (89, KBD | 0x87),        // International1 (Ro)
    (93, KBD | 0x88),        // International2 (Katakana/Hiragana)
    (124, KBD | 0x89),       // International3 (Yen)
    (92, KBD | 0x8A),        // International4 (Henkan)
    (94, KBD | 0x8B),        // International5 (Muhenkan)
    (122, KBD | 0x90),       // Lang1 (Hangul)
    (123, KBD | 0x91),       // Lang2 (Hanja)
    (90, KBD | 0x92),        // Lang3 (Katakana)
    (91, KBD | 0x93),        // Lang4 (Hiragana)
    (85, KBD | 0x94),        // Lang5 (Zenkaku/Hankaku)
    (29, KBD | 0xE0),        // Left Ctrl
    (42, KBD | 0xE1),        // Left Shift
    (56, KBD | 0xE2),        // Left Alt
    (125, KBD | 0xE3),       // Left Meta
    (97, KBD | 0xE4),        // Right Ctrl
    (54, KBD | 0xE5),        // Right Shift
    (100, KBD | 0xE6),       // Right Alt
    (126, KBD | 0xE7),       // Right Meta
    (163, CONSUMER | 0xB5),  // Scan Next Track
    (165, CONSUMER | 0xB6),  // Scan Previous Track
    (166, CONSUMER | 0xB7),  // Stop
    (164, CONSUMER | 0xCD),  // Play/Pause
    (113, CONSUMER | 0xE2),  // Mute
    (115, CONSUMER | 0xE9),  // Volume Increment
    (114, CONSUMER | 0xEA),  // Volume Decrement
    (155, CONSUMER | 0x18A), // AL Email Reader
    (140, CONSUMER | 0x192), // AL Calculator
    (172, CONSUMER | 0x223), // AC Home (WWW home)
];

pub(super) fn hid_from_evdev(code: u16) -> Option<u32> {
    KEYS.iter()
        .find(|&&(evdev, _)| evdev == code)
        .map(|&(_, hid)| hid)
}

pub(super) fn evdev_from_hid(usage: u32) -> Option<u16> {
    KEYS.iter()
        .find(|&&(_, hid)| hid == usage)
        .map(|&(evdev, _)| evdev)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evdev_from_hid_every_entry_round_trips() {
        // Arrange
        let table = KEYS;

        for &(evdev, hid) in table {
            // Act
            let back = hid_from_evdev(evdev).and_then(evdev_from_hid);

            // Assert
            assert_eq!(back, Some(evdev), "evdev {evdev} / usage {hid:#x}");
            assert_eq!(hid_from_evdev(evdev), Some(hid));
        }
    }

    #[test]
    fn keys_table_has_no_duplicate_codes_or_usages() {
        // Arrange
        let mut codes: Vec<u16> = KEYS.iter().map(|&(evdev, _)| evdev).collect();
        let mut usages: Vec<u32> = KEYS.iter().map(|&(_, hid)| hid).collect();

        // Act
        codes.sort_unstable();
        usages.sort_unstable();

        // Assert
        assert!(codes.windows(2).all(|pair| pair[0] != pair[1]));
        assert!(usages.windows(2).all(|pair| pair[0] != pair[1]));
    }

    #[test]
    fn hid_from_evdev_known_keys_match_usage_tables() {
        // Arrange
        let expected = [
            (30, 0x0007_0004),
            (28, 0x0007_0028),
            (125, 0x0007_00E3),
            (113, 0x000C_00E2),
        ];

        for (evdev, usage) in expected {
            // Act
            let hid = hid_from_evdev(evdev);

            // Assert
            assert_eq!(hid, Some(usage), "evdev {evdev}");
        }
    }
}
