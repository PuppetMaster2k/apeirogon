// SPDX-License-Identifier: GPL-3.0-or-later

//! A channel-owning thread injects marked input and reports actual pointer positions.

use std::io;
use std::mem::size_of;
use std::thread;

use a7n_proto::v1::{InputEvent, KeyEvent, input_event};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::{Receiver, Sender};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL,
    MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput, VK_PAUSE,
};

use super::{Bounds, INJECTION_MARKER, keymap, mouse, pointer_position};
use crate::input::scroll::ScrollAccumulator;
use crate::input::{Captured, Inject, InputError};

const ABSOLUTE_MAX: i64 = 65_535;
const PAUSE_USAGE: u32 = 0x0007_0048;

pub(super) fn spawn(
    bounds: Bounds,
    commands: Receiver<Inject>,
    pointer: Sender<Captured>,
) -> Result<(), InputError> {
    thread::Builder::new()
        .name("a7n-windows-inject".to_owned())
        .spawn(move || {
            if run(bounds, commands, &pointer).is_err() {
                eprintln!("kind=input_inject_failed");
            }
        })
        .map(drop)
        .map_err(InputError::Thread)
}

fn run(
    bounds: Bounds,
    mut commands: Receiver<Inject>,
    pointer: &Sender<Captured>,
) -> io::Result<()> {
    let mut scroll = ScrollAccumulator::default();
    while let Some(command) = commands.blocking_recv() {
        let (dx, dy) = match command {
            Inject::Event(event) => {
                event_input(&event, &mut scroll)?;
                continue;
            }
            Inject::Motion { dx, dy } => {
                let current = pointer_position()?;
                warp(
                    bounds,
                    current.x.saturating_add(dx),
                    current.y.saturating_add(dy),
                )?;
                (dx, dy)
            }
            Inject::Warp { x, y } => {
                warp(bounds, x, y)?;
                (0, 0)
            }
        };
        let point = pointer_position()?;
        match pointer.try_send(Captured::Pointer {
            x: point.x,
            y: point.y,
            dx,
            dy,
        }) {
            Err(TrySendError::Closed(_)) => return Ok(()),
            Ok(()) | Err(TrySendError::Full(_)) => {}
        }
    }
    Ok(())
}

fn event_input(event: &InputEvent, scroll: &mut ScrollAccumulator) -> io::Result<()> {
    match &event.event {
        Some(input_event::Event::Key(key)) => {
            if let Some(input) = key_input(key) {
                send(&input)?;
            }
        }
        Some(input_event::Event::Button(button)) => {
            if let Some((flags, data)) = mouse::button_input(button.button(), button.down) {
                send(&mouse_input(flags, data, 0, 0))?;
            }
        }
        Some(input_event::Event::Scroll(event)) => {
            let (dx, dy) = scroll.push(event.dx, event.dy, event.unit());
            for (delta, flags) in [(dx, MOUSEEVENTF_HWHEEL), (-dy, MOUSEEVENTF_WHEEL)] {
                if delta != 0 {
                    let data =
                        u32::from_ne_bytes((delta * i32::from(mouse::WHEEL_UNITS)).to_ne_bytes());
                    send(&mouse_input(flags, data, 0, 0))?;
                }
            }
        }
        // Physical usages are the typing contract; text/gestures have no
        // scan-code equivalent, as in the X11 backend.
        Some(input_event::Event::Text(_) | input_event::Event::Gesture(_)) | None => {}
    }
    Ok(())
}

fn key_input(key: &KeyEvent) -> Option<INPUT> {
    let (vk, scan, mut flags) = if key.usage == PAUSE_USAGE {
        // KEYEVENTF_EXTENDEDKEY represents E0 only; Pause requires the E1 sequence.
        (VK_PAUSE, 0, 0)
    } else if let Some(vk) = keymap::media_from_hid(key.usage) {
        (vk, 0, 0)
    } else {
        let (scan, extended) = keymap::scan_from_hid(key.usage)?;
        (
            0,
            scan,
            KEYEVENTF_SCANCODE | if extended { KEYEVENTF_EXTENDEDKEY } else { 0 },
        )
    };
    if !key.down {
        flags |= KEYEVENTF_KEYUP;
    }
    Some(INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INJECTION_MARKER,
            },
        },
    })
}

fn warp(bounds: Bounds, x: i32, y: i32) -> io::Result<()> {
    let dx = absolute_axis(x, bounds.x, bounds.width);
    let dy = absolute_axis(y, bounds.y, bounds.height);
    send(&mouse_input(
        MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
        0,
        dx,
        dy,
    ))
}

fn absolute_axis(position: i32, origin: i32, length: u32) -> i32 {
    let max = i64::from(length.saturating_sub(1));
    if max == 0 {
        return 0;
    }
    let offset = (i64::from(position) - i64::from(origin)).clamp(0, max);
    i32::try_from(offset * ABSOLUTE_MAX / max).unwrap_or_default()
}

fn mouse_input(flags: u32, data: u32, dx: i32, dy: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INJECTION_MARKER,
            },
        },
    }
}

fn send(input: &INPUT) -> io::Result<()> {
    let size = i32::try_from(size_of::<INPUT>()).map_err(io::Error::other)?;
    // SAFETY: input points to a fully initialized INPUT with its union member
    // matching type; count and element size describe exactly that one value.
    if unsafe { SendInput(1, input, size) } != 1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_axis_negative_origin_and_outside_positions_are_clamped() {
        // Arrange
        let positions = [i32::MIN, -1920, -1, i32::MAX];

        // Act
        let normalized = positions.map(|x| absolute_axis(x, -1920, 1920));

        // Assert
        assert_eq!(normalized, [0, 0, 65_535, 65_535]);
        assert_eq!(absolute_axis(10, 10, 1), 0);
    }

    #[test]
    fn key_input_extended_release_and_media_are_marked_with_correct_flags() {
        // Arrange
        let cases = [
            (
                0x0007_00E4,
                0,
                0x1D,
                KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP,
            ),
            (
                0x000C_00E9,
                windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_VOLUME_UP,
                0,
                KEYEVENTF_KEYUP,
            ),
            (PAUSE_USAGE, VK_PAUSE, 0, KEYEVENTF_KEYUP),
        ];

        for (usage, vk, scan, flags) in cases {
            // Act
            let input = key_input(&KeyEvent {
                usage,
                down: false,
                character: String::new(),
            });

            // Assert
            let input = input.expect("known usage is mapped");
            assert_eq!(input.r#type, INPUT_KEYBOARD);
            // SAFETY: key_input constructs INPUT_KEYBOARD with the ki member.
            let keyboard = unsafe { input.Anonymous.ki };
            assert_eq!(
                (keyboard.wVk, keyboard.wScan, keyboard.dwFlags),
                (vk, scan, flags)
            );
            assert_eq!(keyboard.dwExtraInfo, INJECTION_MARKER);
        }
    }
}
