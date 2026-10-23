// SPDX-License-Identifier: GPL-3.0-or-later

//! Pointer button and wheel conventions shared by capture and injection.

use a7n_proto::v1::{Button, ButtonEvent, ScrollEvent, ScrollUnit, input_event};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    WHEEL_DELTA, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL,
    WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_XBUTTONDOWN, WM_XBUTTONUP, XBUTTON1, XBUTTON2,
};

#[allow(
    clippy::cast_possible_truncation,
    reason = "Windows WHEEL_DELTA is 120 and fits in i16"
)]
pub(super) const WHEEL_UNITS: i16 = WHEEL_DELTA as i16;

pub(super) fn translate(message: u32, data: u32) -> Option<input_event::Event> {
    let (button, down) = match message {
        WM_LBUTTONDOWN => (Button::Left, true),
        WM_LBUTTONUP => (Button::Left, false),
        WM_RBUTTONDOWN => (Button::Right, true),
        WM_RBUTTONUP => (Button::Right, false),
        WM_MBUTTONDOWN => (Button::Middle, true),
        WM_MBUTTONUP => (Button::Middle, false),
        WM_XBUTTONDOWN | WM_XBUTTONUP => {
            let button = match u16::try_from(data >> u16::BITS).ok()? {
                XBUTTON1 => Button::Back,
                XBUTTON2 => Button::Forward,
                _ => return None,
            };
            (button, message == WM_XBUTTONDOWN)
        }
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
            let bytes = data.to_be_bytes();
            let delta =
                f32::from(i16::from_be_bytes([bytes[0], bytes[1]])) / f32::from(WHEEL_UNITS);
            let (dx, dy) = if message == WM_MOUSEWHEEL {
                (0.0, -delta)
            } else {
                (delta, 0.0)
            };
            return Some(input_event::Event::Scroll(ScrollEvent {
                dx,
                dy,
                unit: ScrollUnit::Line.into(),
            }));
        }
        _ => return None,
    };
    Some(input_event::Event::Button(ButtonEvent {
        button: button.into(),
        down,
    }))
}

pub(super) fn button_input(button: Button, down: bool) -> Option<(u32, u32)> {
    let (press, release, data) = match button {
        Button::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, 0),
        Button::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, 0),
        Button::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, 0),
        Button::Back => (MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, XBUTTON1),
        Button::Forward => (MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, XBUTTON2),
        Button::Unspecified => return None,
    };
    Some((if down { press } else { release }, u32::from(data)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translate_wheels_follow_protocol_direction_and_keep_fractional_lines() {
        // Arrange
        let cases = [
            (WM_MOUSEWHEEL, 120_i16, (0.0, -1.0)),
            (WM_MOUSEWHEEL, -60, (0.0, 0.5)),
            (WM_MOUSEHWHEEL, 120, (1.0, 0.0)),
        ];

        for (message, delta, expected) in cases {
            // Act
            let event = translate(
                message,
                u32::from(u16::from_ne_bytes(delta.to_ne_bytes())) << u16::BITS,
            );

            // Assert
            assert!(matches!(event, Some(input_event::Event::Scroll(s))
                if (s.dx, s.dy) == expected && s.unit() == ScrollUnit::Line));
        }
    }

    #[test]
    fn translate_buttons_preserves_all_five_identities_and_releases() {
        // Arrange
        let cases = [
            (WM_LBUTTONUP, 0, Button::Left),
            (WM_RBUTTONUP, 0, Button::Right),
            (WM_MBUTTONUP, 0, Button::Middle),
            (WM_XBUTTONUP, XBUTTON1, Button::Back),
            (WM_XBUTTONUP, XBUTTON2, Button::Forward),
        ];

        for (message, data, button) in cases {
            // Act
            let event = translate(message, u32::from(data) << u16::BITS);

            // Assert
            assert!(
                matches!(event, Some(input_event::Event::Button(b)) if b.button() == button && !b.down)
            );
            assert!(button_input(button, false).is_some());
        }
    }
}
