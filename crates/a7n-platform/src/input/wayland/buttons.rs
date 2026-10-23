// SPDX-License-Identifier: GPL-3.0-or-later

//! Linux evdev button codes (`input-event-codes.h`) <-> protocol buttons.

use a7n_proto::v1::Button;

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;
/// The rocker button under the thumb that mice use for "back".
const BTN_SIDE: u32 = 0x113;
/// The second thumb button, used for "forward".
const BTN_EXTRA: u32 = 0x114;

pub(super) fn button_from_evdev(code: u32) -> Option<Button> {
    match code {
        BTN_LEFT => Some(Button::Left),
        BTN_RIGHT => Some(Button::Right),
        BTN_MIDDLE => Some(Button::Middle),
        BTN_SIDE => Some(Button::Back),
        BTN_EXTRA => Some(Button::Forward),
        _ => None,
    }
}

pub(super) fn evdev_from_button(button: Button) -> Option<u32> {
    match button {
        Button::Unspecified => None,
        Button::Left => Some(BTN_LEFT),
        Button::Right => Some(BTN_RIGHT),
        Button::Middle => Some(BTN_MIDDLE),
        Button::Back => Some(BTN_SIDE),
        Button::Forward => Some(BTN_EXTRA),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evdev_from_button_every_button_round_trips() {
        // Arrange
        let buttons = [
            Button::Left,
            Button::Right,
            Button::Middle,
            Button::Back,
            Button::Forward,
        ];

        for button in buttons {
            // Act
            let back = evdev_from_button(button).and_then(button_from_evdev);

            // Assert
            assert_eq!(back, Some(button), "{button:?}");
        }
    }
}
