// SPDX-License-Identifier: GPL-3.0-or-later

use std::io;

use std::thread;

use a7n_proto::v1::{Button, InputEvent, input_event};
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
use objc2_core_foundation::{CFRetained, CGPoint};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton,
    CGScrollEventUnit,
};
use objc2_foundation::NSPoint;
use tokio::sync::mpsc::{Receiver, Sender};

use super::{INJECTION_MARKER, MEDIA_DOWN, MEDIA_SUBTYPE, MEDIA_UP, keymap, pointer_position};
use crate::input::scroll::ScrollAccumulator;
use crate::input::{Bounds, Captured, Inject, InputError};

const KEY_COUNT: usize = 128;
const BUTTON_COUNT: usize = 5;
const SCROLL_AXES: u32 = 2;

struct State {
    keys: [bool; KEY_COUNT],
    buttons: [bool; BUTTON_COUNT],
    scroll: ScrollAccumulator,
}

pub(super) fn spawn(
    bounds: Bounds,
    commands: Receiver<Inject>,
    pointer: Sender<Captured>,
) -> Result<(), InputError> {
    thread::Builder::new()
        .name("a7n-macos-inject".to_owned())
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
    let mut state = State {
        keys: [false; KEY_COUNT],
        buttons: [false; BUTTON_COUNT],
        scroll: ScrollAccumulator::default(),
    };
    while let Some(command) = commands.blocking_recv() {
        let (point, dx, dy) = match command {
            Inject::Event(event) => {
                state.event(&event)?;
                continue;
            }
            Inject::Motion { dx, dy } => {
                let point = pointer_position()?;
                (
                    CGPoint::new(point.x + f64::from(dx), point.y + f64::from(dy)),
                    dx,
                    dy,
                )
            }
            Inject::Warp { x, y } => (CGPoint::new(f64::from(x), f64::from(y)), 0, 0),
        };
        let point = CGPoint::new(
            point.x.clamp(
                f64::from(bounds.x),
                f64::from(bounds.x) + f64::from(bounds.width) - 1.0,
            ),
            point.y.clamp(
                f64::from(bounds.y),
                f64::from(bounds.y) + f64::from(bounds.height) - 1.0,
            ),
        );
        state.motion(point, dx, dy)?;
        let actual = pointer_position()?;
        let event = Captured::Pointer {
            x: coordinate(actual.x),
            y: coordinate(actual.y),
            dx,
            dy,
        };
        if matches!(
            pointer.try_send(event),
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_))
        ) {
            break;
        }
    }
    Ok(())
}

impl State {
    fn flags(&self) -> CGEventFlags {
        let flags = self
            .keys
            .iter()
            .enumerate()
            .filter(|&(_, held)| *held)
            .fold(0, |flags, (key, _)| {
                flags | keymap::modifier_flags(u16::try_from(key).expect("key array fits u16"))
            });
        CGEventFlags(flags)
    }

    fn post(&self, event: Option<CFRetained<CGEvent>>) -> io::Result<()> {
        let event = event.ok_or_else(|| io::Error::other("cannot create input event"))?;
        CGEvent::set_flags(Some(&event), self.flags());
        CGEvent::set_integer_value_field(
            Some(&event),
            CGEventField::EventSourceUserData,
            INJECTION_MARKER,
        );
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
        Ok(())
    }

    fn event(&mut self, event: &InputEvent) -> io::Result<()> {
        match &event.event {
            Some(input_event::Event::Key(key)) => {
                if let Some(code) = keymap::key_from_hid(key.usage) {
                    self.keys[usize::from(code)] = key.down;
                    self.post(CGEvent::new_keyboard_event(None, code, key.down))?;
                } else if let Some(code) = keymap::media_from_hid(key.usage) {
                    self.media(code, key.down)?;
                }
            }
            Some(input_event::Event::Button(button)) => {
                let index = match button.button() {
                    Button::Left => 0,
                    Button::Right => 1,
                    Button::Middle => 2,
                    Button::Back => 3,
                    Button::Forward => 4,
                    Button::Unspecified => return Ok(()),
                };
                self.buttons[index] = button.down;
                let ty = match (index, button.down) {
                    (0, true) => CGEventType::LeftMouseDown,
                    (0, false) => CGEventType::LeftMouseUp,
                    (1, true) => CGEventType::RightMouseDown,
                    (1, false) => CGEventType::RightMouseUp,
                    (_, true) => CGEventType::OtherMouseDown,
                    (_, false) => CGEventType::OtherMouseUp,
                };
                let button = CGMouseButton(u32::try_from(index).expect("button array fits u32"));
                self.post(CGEvent::new_mouse_event(
                    None,
                    ty,
                    pointer_position()?,
                    button,
                ))?;
            }
            Some(input_event::Event::Scroll(scroll)) => {
                let (dx, dy) = self.scroll.push(scroll.dx, scroll.dy, scroll.unit());
                if dx != 0 || dy != 0 {
                    self.post(CGEvent::new_scroll_wheel_event2(
                        None,
                        CGScrollEventUnit::Line,
                        SCROLL_AXES,
                        -dy,
                        dx,
                        0,
                    ))?;
                }
            }
            Some(input_event::Event::Text(_) | input_event::Event::Gesture(_)) | None => {}
        }
        Ok(())
    }

    fn motion(&self, point: CGPoint, dx: i32, dy: i32) -> io::Result<()> {
        let (ty, button) = match self.buttons.iter().position(|held| *held) {
            Some(0) => (CGEventType::LeftMouseDragged, CGMouseButton::Left),
            Some(1) => (CGEventType::RightMouseDragged, CGMouseButton::Right),
            Some(index) => (
                CGEventType::OtherMouseDragged,
                CGMouseButton(u32::try_from(index).expect("button array fits u32")),
            ),
            None => (CGEventType::MouseMoved, CGMouseButton::Left),
        };
        let event = CGEvent::new_mouse_event(None, ty, point, button)
            .ok_or_else(|| io::Error::other("cannot create pointer event"))?;
        CGEvent::set_integer_value_field(
            Some(&event),
            CGEventField::MouseEventDeltaX,
            i64::from(dx),
        );
        CGEvent::set_integer_value_field(
            Some(&event),
            CGEventField::MouseEventDeltaY,
            i64::from(dy),
        );
        self.post(Some(event))
    }

    fn media(&self, code: u16, down: bool) -> io::Result<()> {
        autoreleasepool(|_| {
            let data = (isize::try_from(code).expect("media code fits isize") << 16)
                | (isize::try_from(if down { MEDIA_DOWN } else { MEDIA_UP })
                    .expect("media state fits isize")
                    << 8);
            let event = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
                NSEventType::SystemDefined, NSPoint::new(0.0, 0.0), NSEventModifierFlags::empty(), 0.0, 0, None, MEDIA_SUBTYPE, data, -1,
            ).ok_or_else(|| io::Error::other("cannot create media event"))?;
            let event = event
                .CGEvent()
                .ok_or_else(|| io::Error::other("cannot convert media event"))?;
            CGEvent::set_flags(Some(&event), self.flags());
            CGEvent::set_integer_value_field(
                Some(&event),
                CGEventField::EventSourceUserData,
                INJECTION_MARKER,
            );
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
            Ok(())
        })
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "float-to-int casts saturate desktop coordinates"
)]
fn coordinate(value: f64) -> i32 {
    value as i32
}
