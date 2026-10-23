// SPDX-License-Identifier: GPL-3.0-or-later

//! X keycodes are evdev codes plus [`EVDEV_KEYCODE_OFFSET`].

use crate::input::evdev_keymap::{evdev_from_hid, hid_from_evdev};

/// X keycode minus evdev code, fixed by the xkb `evdev` rules.
const EVDEV_KEYCODE_OFFSET: u32 = 8;

/// Unknown keycodes (including those below the evdev range) yield `None`.
pub(super) fn hid_from_keycode(keycode: u32) -> Option<u32> {
    let evdev = keycode.checked_sub(EVDEV_KEYCODE_OFFSET)?;
    hid_from_evdev(u16::try_from(evdev).ok()?)
}

pub(super) fn keycode_from_hid(usage: u32) -> Option<u8> {
    let keycode = u32::from(evdev_from_hid(usage)?) + EVDEV_KEYCODE_OFFSET;
    u8::try_from(keycode).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keycode_from_hid_letter_a_adds_x_offset() {
        // Arrange
        let usage = 0x0007_0004;

        // Act
        let keycode = keycode_from_hid(usage);

        // Assert
        assert_eq!(keycode, Some(38));
        assert_eq!(hid_from_keycode(38), Some(usage));
    }

    #[test]
    fn hid_from_keycode_below_evdev_range_is_unknown() {
        // Arrange
        let reserved_keycode = 7;

        // Act
        let usage = hid_from_keycode(reserved_keycode);

        // Assert
        assert_eq!(usage, None);
    }
}
