// SPDX-License-Identifier: GPL-3.0-or-later

//! Mapping between X pointer button numbers and protocol buttons.
//!
//! The protocol numbers right and middle the other way round from X.

use a7n_proto::v1::Button;

const X_BUTTON_LEFT: u8 = 1;
const X_BUTTON_MIDDLE: u8 = 2;
const X_BUTTON_RIGHT: u8 = 3;
const X_SCROLL_UP: u8 = 4;
const X_SCROLL_DOWN: u8 = 5;
const X_SCROLL_LEFT: u8 = 6;
const X_SCROLL_RIGHT: u8 = 7;
const X_BUTTON_BACK: u8 = 8;
const X_BUTTON_FORWARD: u8 = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PointerButton {
    Click(Button),
    /// One wheel notch; positive `dy` is towards the end of the document.
    Scroll {
        dx: i8,
        dy: i8,
    },
}

pub(super) fn classify_x_button(detail: u32) -> Option<PointerButton> {
    let button = match u8::try_from(detail).ok()? {
        X_BUTTON_LEFT => PointerButton::Click(Button::Left),
        X_BUTTON_MIDDLE => PointerButton::Click(Button::Middle),
        X_BUTTON_RIGHT => PointerButton::Click(Button::Right),
        X_BUTTON_BACK => PointerButton::Click(Button::Back),
        X_BUTTON_FORWARD => PointerButton::Click(Button::Forward),
        X_SCROLL_UP => PointerButton::Scroll { dx: 0, dy: -1 },
        X_SCROLL_DOWN => PointerButton::Scroll { dx: 0, dy: 1 },
        X_SCROLL_LEFT => PointerButton::Scroll { dx: -1, dy: 0 },
        X_SCROLL_RIGHT => PointerButton::Scroll { dx: 1, dy: 0 },
        _ => return None,
    };
    Some(button)
}

pub(super) fn x_button_from(button: Button) -> Option<u8> {
    match button {
        Button::Unspecified => None,
        Button::Left => Some(X_BUTTON_LEFT),
        Button::Middle => Some(X_BUTTON_MIDDLE),
        Button::Right => Some(X_BUTTON_RIGHT),
        Button::Back => Some(X_BUTTON_BACK),
        Button::Forward => Some(X_BUTTON_FORWARD),
    }
}

/// X buttons to click, in order, for whole wheel notches.
pub(super) fn scroll_clicks(dx: i32, dy: i32) -> impl Iterator<Item = u8> {
    let vertical = if dy < 0 { X_SCROLL_UP } else { X_SCROLL_DOWN };
    let horizontal = if dx < 0 {
        X_SCROLL_LEFT
    } else {
        X_SCROLL_RIGHT
    };
    std::iter::repeat_n(vertical, dy.unsigned_abs() as usize)
        .chain(std::iter::repeat_n(horizontal, dx.unsigned_abs() as usize))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_x_button_right_and_middle_are_swapped_against_protocol_numbers() {
        // Arrange
        let (middle, right) = (2, 3);

        // Act
        let buttons = [middle, right].map(classify_x_button);

        // Assert
        assert_eq!(
            buttons,
            [
                Some(PointerButton::Click(Button::Middle)),
                Some(PointerButton::Click(Button::Right))
            ]
        );
    }

    #[test]
    fn scroll_clicks_negative_dy_clicks_scroll_up() {
        // Arrange
        let (dx, dy) = (1, -2);

        // Act
        let clicks: Vec<u8> = scroll_clicks(dx, dy).collect();

        // Assert
        assert_eq!(clicks, [X_SCROLL_UP, X_SCROLL_UP, X_SCROLL_RIGHT]);
    }
}
