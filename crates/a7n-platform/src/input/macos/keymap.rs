// SPDX-License-Identifier: GPL-3.0-or-later

//! Physical USB keyboard usages and macOS virtual keys (independent of layout).

const KEYBOARD: u32 = 0x0007_0000;
const CONSUMER: u32 = 0x000C_0000;
static KEYS: &[(u16, u16)] = &[
    (0x04, 0x00),
    (0x05, 0x0B),
    (0x06, 0x08),
    (0x07, 0x02),
    (0x08, 0x0E),
    (0x09, 0x03),
    (0x0A, 0x05),
    (0x0B, 0x04),
    (0x0C, 0x22),
    (0x0D, 0x26),
    (0x0E, 0x28),
    (0x0F, 0x25),
    (0x10, 0x2E),
    (0x11, 0x2D),
    (0x12, 0x1F),
    (0x13, 0x23),
    (0x14, 0x0C),
    (0x15, 0x0F),
    (0x16, 0x01),
    (0x17, 0x11),
    (0x18, 0x20),
    (0x19, 0x09),
    (0x1A, 0x0D),
    (0x1B, 0x07),
    (0x1C, 0x10),
    (0x1D, 0x06),
    (0x1E, 0x12),
    (0x1F, 0x13),
    (0x20, 0x14),
    (0x21, 0x15),
    (0x22, 0x17),
    (0x23, 0x16),
    (0x24, 0x1A),
    (0x25, 0x1C),
    (0x26, 0x19),
    (0x27, 0x1D),
    (0x28, 0x24),
    (0x29, 0x35),
    (0x2A, 0x33),
    (0x2B, 0x30),
    (0x2C, 0x31),
    (0x2D, 0x1B),
    (0x2E, 0x18),
    (0x2F, 0x21),
    (0x30, 0x1E),
    (0x31, 0x2A),
    (0x33, 0x29),
    (0x34, 0x27),
    (0x35, 0x32),
    (0x36, 0x2B),
    (0x37, 0x2F),
    (0x38, 0x2C),
    (0x39, 0x39),
    (0x3A, 0x7A),
    (0x3B, 0x78),
    (0x3C, 0x63),
    (0x3D, 0x76),
    (0x3E, 0x60),
    (0x3F, 0x61),
    (0x40, 0x62),
    (0x41, 0x64),
    (0x42, 0x65),
    (0x43, 0x6D),
    (0x44, 0x67),
    (0x45, 0x6F),
    (0x49, 0x72),
    (0x4A, 0x73),
    (0x4B, 0x74),
    (0x4C, 0x75),
    (0x4D, 0x77),
    (0x4E, 0x79),
    (0x4F, 0x7C),
    (0x50, 0x7B),
    (0x51, 0x7D),
    (0x52, 0x7E),
    (0x53, 0x47),
    (0x54, 0x4B),
    (0x55, 0x43),
    (0x56, 0x4E),
    (0x57, 0x45),
    (0x58, 0x4C),
    (0x59, 0x53),
    (0x5A, 0x54),
    (0x5B, 0x55),
    (0x5C, 0x56),
    (0x5D, 0x57),
    (0x5E, 0x58),
    (0x5F, 0x59),
    (0x60, 0x5B),
    (0x61, 0x5C),
    (0x62, 0x52),
    (0x63, 0x41),
    (0x64, 0x0A),
    (0x67, 0x51),
    (0x68, 0x69),
    (0x69, 0x6B),
    (0x6A, 0x71),
    (0x6B, 0x6A),
    (0x6C, 0x40),
    (0x6D, 0x4F),
    (0x6E, 0x50),
    (0x6F, 0x5A),
    (0x87, 0x5E),
    (0x89, 0x5D),
    (0x90, 0x68),
    (0x91, 0x66),
    (0xE0, 0x3B),
    (0xE1, 0x38),
    (0xE2, 0x3A),
    (0xE3, 0x37),
    (0xE4, 0x3E),
    (0xE5, 0x3C),
    (0xE6, 0x3D),
    (0xE7, 0x36),
];

pub(super) fn hid_from_key(code: u16) -> Option<u32> {
    KEYS.iter()
        .find(|&&(_, key)| key == code)
        .map(|&(hid, _)| KEYBOARD | u32::from(hid))
}

pub(super) fn key_from_hid(usage: u32) -> Option<u16> {
    KEYS.iter()
        .find(|&&(hid, _)| KEYBOARD | u32::from(hid) == usage)
        .map(|&(_, key)| key)
}

// Device-specific bits distinguish release of one side while the other stays held.
pub(super) fn modifier_mask(code: u16) -> Option<u64> {
    match code {
        59 => Some(0x0001),
        62 => Some(0x2000),
        56 => Some(0x0002),
        60 => Some(0x0004),
        55 => Some(0x0008),
        54 => Some(0x0010),
        58 => Some(0x0020),
        61 => Some(0x0040),
        57 => Some(0x0001_0000),
        _ => None,
    }
}

pub(super) fn modifier_down(code: u16, flags: u64) -> Option<bool> {
    modifier_mask(code).map(|mask| flags & mask != 0)
}

pub(super) fn modifier_flags(code: u16) -> u64 {
    let general = match code {
        59 | 62 => 0x0004_0000,
        56 | 60 => 0x0002_0000,
        55 | 54 => 0x0010_0000,
        58 | 61 => 0x0008_0000,
        57 => 0x0001_0000,
        _ => 0,
    };
    general | modifier_mask(code).unwrap_or_default()
}

static MEDIA: &[(u16, u16)] = &[
    (0xE9, 0),
    (0xEA, 1),
    (0xE2, 7),
    (0xCD, 16),
    (0xB5, 17),
    (0xB6, 18),
];

pub(super) fn hid_from_media(code: u16) -> Option<u32> {
    MEDIA
        .iter()
        .find(|&&(_, key)| key == code)
        .map(|&(hid, _)| CONSUMER | u32::from(hid))
}

pub(super) fn media_from_hid(usage: u32) -> Option<u16> {
    MEDIA
        .iter()
        .find(|&&(hid, _)| CONSUMER | u32::from(hid) == usage)
        .map(|&(_, key)| key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_from_hid_keyboard_and_media_tables_round_trip() {
        // Arrange
        let keys = KEYS;

        for &(hid, key) in keys {
            // Act
            let usage = hid_from_key(key);

            // Assert
            assert_eq!(usage, Some(KEYBOARD | u32::from(hid)));
            assert_eq!(usage.and_then(key_from_hid), Some(key));
        }
        for &(hid, key) in MEDIA {
            // Act
            let usage = hid_from_media(key);

            // Assert
            assert_eq!(usage, Some(CONSUMER | u32::from(hid)));
            assert_eq!(usage.and_then(media_from_hid), Some(key));
        }
    }

    #[test]
    fn modifier_down_other_side_held_does_not_hide_release() {
        // Arrange
        let codes = [59, 62, 56, 60, 55, 54, 58, 61, 57];

        for code in codes {
            // Act
            let down = modifier_down(code, modifier_flags(code));
            let up = modifier_down(code, !modifier_mask(code).expect("modifier"));

            // Assert
            assert_eq!((down, up), (Some(true), Some(false)));
        }
    }
}
