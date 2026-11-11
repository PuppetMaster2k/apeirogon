// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;
use std::ffi::c_void;
use std::io;
use std::ptr::{NonNull, null_mut};
use std::sync::mpsc as sync;
use std::thread;
use std::time::{Duration, Instant};

use a7n_proto::v1::{
    Button, ButtonEvent, InputEvent, KeyEvent, ScrollEvent, ScrollUnit, input_event,
};
use objc2::rc::autoreleasepool;
use objc2_app_kit::NSEvent;
use objc2_core_foundation::{CFRunLoop, CGPoint, kCFRunLoopDefaultMode};
use objc2_core_graphics::{
    CGAssociateMouseAndMouseCursorPosition, CGDisplayHideCursor, CGDisplayShowCursor, CGEvent,
    CGEventField, CGEventTapProxy, CGEventType, CGMainDisplayID,
};
use tokio::sync::mpsc::error::{TryRecvError, TrySendError};
use tokio::sync::mpsc::{Receiver, Sender};

use super::{
    INJECTION_MARKER, MEDIA_DOWN, MEDIA_EVENT, MEDIA_SUBTYPE, MEDIA_UP, check, keymap,
    pointer_position,
};
use crate::input::local_key::LocalKey;
use crate::input::{CaptureControl, Captured, DeadMan, InputError};
use crate::permissions;

mod gestures;
mod tap;

use gestures::Swipes;

use tap::Tap;

const WAKE_SECONDS: f64 = 0.05;
const KEY_COUNT: usize = 128;
const PERMISSION_CHECK_INTERVAL: Duration = Duration::from_secs(1);

struct State {
    events: Sender<Captured>,
    deadman: DeadMan,
    grabbed: bool,
    queue_full: bool,
    disabled: bool,
    permission_checked: Instant,
    keys: [LocalKey; KEY_COUNT],
    anchor: CGPoint,
    swipes: Swipes,
}

pub(super) fn spawn(
    events: Sender<Captured>,
    control: Receiver<CaptureControl>,
) -> Result<(), InputError> {
    let (ready, result) = sync::channel();
    thread::Builder::new()
        .name("a7n-macos-capture".to_owned())
        .spawn(move || {
            let state = RefCell::new(State {
                events,
                deadman: DeadMan::new(Instant::now()),
                grabbed: false,
                queue_full: false,
                disabled: false,
                permission_checked: Instant::now(),
                keys: [LocalKey::default(); KEY_COUNT],
                anchor: CGPoint::new(0.0, 0.0),
                swipes: Swipes::default(),
            });
            let outcome = run(&state, control, &ready);
            state.borrow_mut().grab(false);
            if let Err(error) = outcome {
                let _ = ready.send(Err(error));
            }
        })
        .map_err(InputError::Thread)?;
    result
        .recv()
        .map_err(|error| io::Error::other(error.to_string()))??;
    Ok(())
}

fn run(
    state: &RefCell<State>,
    mut control: Receiver<CaptureControl>,
    ready: &sync::Sender<io::Result<()>>,
) -> io::Result<()> {
    let mut tap = Tap::install(state, false)?;
    // SAFETY: Core Foundation exports this immutable mode constant for the process lifetime.
    let mode = unsafe { kCFRunLoopDefaultMode };
    if ready.send(Ok(())).is_err() {
        return Ok(());
    }
    while tick(state, &mut tap, &mut control)? {
        // The callback may access state only during this synchronous dispatch.
        CFRunLoop::run_in_mode(mode, WAKE_SECONDS, false);
    }
    Ok(())
}

fn tick(
    state: &RefCell<State>,
    tap: &mut Tap,
    control: &mut Receiver<CaptureControl>,
) -> io::Result<bool> {
    let now = Instant::now();
    if state.borrow_mut().permission_check_due(now) && !permissions::missing().is_empty() {
        state.borrow_mut().grab(false);
        return Ok(false);
    }
    // Callbacks can release a grab after a disabled tap or closed event queue.
    if !state.borrow().grabbed {
        tap.set_grab(state, false)?;
    }
    loop {
        match control.try_recv() {
            Ok(CaptureControl::Keepalive) => state.borrow_mut().deadman.keepalive(now),
            Ok(CaptureControl::Grab(grab)) => {
                if grab {
                    state.borrow_mut().deadman.keepalive(now);
                }
                tap.set_grab(state, grab)?;
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => return Ok(false),
        }
    }
    if state.borrow().grabbed && state.borrow().deadman.expired(now) {
        tap.set_grab(state, false)?;
        eprintln!("kind=input_deadman_ungrab");
    }
    let mut state = state.borrow_mut();
    if std::mem::take(&mut state.queue_full) {
        eprintln!("kind=capture_queue_full");
    }
    if std::mem::take(&mut state.disabled) {
        tap.enable();
    }
    Ok(!state.events.is_closed())
}

impl State {
    fn grab(&mut self, grab: bool) {
        if self.grabbed == grab {
            return;
        }
        if grab {
            let Ok(anchor) = pointer_position() else {
                return;
            };
            self.anchor = anchor;
            if check(CGAssociateMouseAndMouseCursorPosition(false)).is_err() {
                return;
            }
            if check(CGDisplayHideCursor(CGMainDisplayID())).is_err() {
                let _ = CGAssociateMouseAndMouseCursorPosition(true);
                return;
            }
        } else {
            let _ = CGAssociateMouseAndMouseCursorPosition(true);
            let _ = CGDisplayShowCursor(CGMainDisplayID());
        }
        self.swipes = Swipes::default();
        self.grabbed = grab;
    }

    fn permission_check_due(&mut self, now: Instant) -> bool {
        if now.duration_since(self.permission_checked) < PERMISSION_CHECK_INTERVAL {
            return false;
        }
        self.permission_checked = now;
        true
    }

    fn emit(&mut self, event: Captured) {
        match self.events.try_send(event) {
            Err(TrySendError::Full(_)) => self.queue_full = true,
            Err(TrySendError::Closed(_)) => self.grab(false),
            Ok(()) => {}
        }
    }

    fn event(&mut self, ty: CGEventType, event: &CGEvent) -> bool {
        if ty == CGEventType::TapDisabledByTimeout || ty == CGEventType::TapDisabledByUserInput {
            self.disabled = true;
            self.grab(false);
            return false;
        }
        if CGEvent::integer_value_field(Some(event), CGEventField::EventSourceUserData)
            == INJECTION_MARKER
        {
            return false;
        }
        let field = |field| CGEvent::integer_value_field(Some(event), field);
        let mut swallow = self.grabbed;
        let translated = match ty {
            CGEventType::KeyDown | CGEventType::KeyUp | CGEventType::FlagsChanged => {
                let Ok(code) = u16::try_from(field(CGEventField::KeyboardEventKeycode)) else {
                    return self.grabbed;
                };
                let down = if ty == CGEventType::FlagsChanged {
                    keymap::modifier_down(code, CGEvent::flags(Some(event)).0).unwrap_or(false)
                } else {
                    ty == CGEventType::KeyDown
                };
                let Some(held) = self.keys.get_mut(usize::from(code)) else {
                    return self.grabbed;
                };
                let repeat = held.held == down;
                swallow = held.swallow(down, self.grabbed);
                if repeat {
                    return swallow;
                }
                keymap::hid_from_key(code).map(|usage| key_event(usage, down))
            }
            CGEventType::MouseMoved
            | CGEventType::LeftMouseDragged
            | CGEventType::RightMouseDragged
            | CGEventType::OtherMouseDragged => {
                let point = if self.grabbed {
                    self.anchor
                } else {
                    CGEvent::location(Some(event))
                };
                self.emit(Captured::Pointer {
                    x: coordinate(point.x),
                    y: coordinate(point.y),
                    dx: delta(field(CGEventField::MouseEventDeltaX)),
                    dy: delta(field(CGEventField::MouseEventDeltaY)),
                });
                None
            }
            CGEventType::ScrollWheel => Some(scroll(event)),
            CGEventType::LeftMouseDown
            | CGEventType::LeftMouseUp
            | CGEventType::RightMouseDown
            | CGEventType::RightMouseUp
            | CGEventType::OtherMouseDown
            | CGEventType::OtherMouseUp => {
                let button = match field(CGEventField::MouseEventButtonNumber) {
                    0 => Button::Left,
                    1 => Button::Right,
                    2 => Button::Middle,
                    3 => Button::Back,
                    4 => Button::Forward,
                    _ => return self.grabbed,
                };
                let down = matches!(
                    ty,
                    CGEventType::LeftMouseDown
                        | CGEventType::RightMouseDown
                        | CGEventType::OtherMouseDown
                );
                Some(input_event::Event::Button(ButtonEvent {
                    button: button.into(),
                    down,
                }))
            }
            _ if ty.0 == gestures::GESTURE_EVENT_TYPE => autoreleasepool(|_| {
                self.swipes
                    .event(event)
                    .map(|gesture| input_event::Event::Gesture(gesture.into()))
            }),
            _ if ty.0 == MEDIA_EVENT => autoreleasepool(|_| media(event)),
            _ => None,
        };
        if let Some(event) = translated {
            self.emit(Captured::Input(InputEvent { event: Some(event) }));
        }
        swallow
    }
}

fn scroll(event: &CGEvent) -> input_event::Event {
    let continuous =
        CGEvent::integer_value_field(Some(event), CGEventField::ScrollWheelEventIsContinuous) != 0;
    let (x, y, unit) = if continuous {
        (
            CGEventField::ScrollWheelEventPointDeltaAxis2,
            CGEventField::ScrollWheelEventPointDeltaAxis1,
            ScrollUnit::Pixel,
        )
    } else {
        (
            CGEventField::ScrollWheelEventDeltaAxis2,
            CGEventField::ScrollWheelEventDeltaAxis1,
            ScrollUnit::Line,
        )
    };
    input_event::Event::Scroll(ScrollEvent {
        dx: scroll_delta(CGEvent::integer_value_field(Some(event), x)),
        dy: -scroll_delta(CGEvent::integer_value_field(Some(event), y)),
        unit: unit.into(),
    })
}

fn key_event(usage: u32, down: bool) -> input_event::Event {
    input_event::Event::Key(KeyEvent {
        usage,
        down,
        character: String::new(),
    })
}

fn media(event: &CGEvent) -> Option<input_event::Event> {
    let event = NSEvent::eventWithCGEvent(event)?;
    if event.subtype().0 != MEDIA_SUBTYPE {
        return None;
    }
    let data = event.data1();
    // Native autorepeat is generated on the receiver.
    if data & 1 != 0 {
        return None;
    }
    let code = u16::try_from((data >> 16) & 0xFFFF).ok()?;
    let state = u16::try_from((data >> 8) & 0xFF).ok()?;
    if state != MEDIA_DOWN && state != MEDIA_UP {
        return None;
    }
    keymap::hid_from_media(code).map(|usage| key_event(usage, state == MEDIA_DOWN))
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "float-to-int casts saturate desktop coordinates"
)]
fn coordinate(value: f64) -> i32 {
    value as i32
}
fn delta(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}
#[allow(
    clippy::cast_precision_loss,
    reason = "scroll fields are bounded to i32 before conversion"
)]
fn scroll_delta(value: i64) -> f32 {
    delta(value) as f32
}

unsafe extern "C-unwind" fn callback(
    _: CGEventTapProxy,
    ty: CGEventType,
    event: NonNull<CGEvent>,
    context: *mut c_void,
) -> *mut CGEvent {
    // SAFETY: install supplies State storage valid until tap invalidation; Core
    // Graphics lends the event for this callback. Dispatch is confined to this thread.
    let swallow = unsafe {
        (&*context.cast::<RefCell<State>>())
            .borrow_mut()
            .event(ty, event.as_ref())
    };
    // Core Graphics ignores this return value for a listen-only tap.
    if swallow { null_mut() } else { event.as_ptr() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_check_due_frequent_ticks_checks_at_most_once_per_second() {
        // Arrange
        let start = Instant::now();
        let (events, _receiver) = tokio::sync::mpsc::channel(1);
        let mut state = State {
            events,
            deadman: DeadMan::new(start),
            grabbed: false,
            queue_full: false,
            disabled: false,
            permission_checked: start,
            keys: [LocalKey::default(); KEY_COUNT],
            anchor: CGPoint::new(0.0, 0.0),
            swipes: Swipes::default(),
        };
        let offsets = [
            Duration::ZERO,
            Duration::from_millis(999),
            PERMISSION_CHECK_INTERVAL,
            PERMISSION_CHECK_INTERVAL,
            Duration::from_millis(1999),
            PERMISSION_CHECK_INTERVAL * 2,
        ];

        // Act
        let checks = offsets.map(|offset| state.permission_check_due(start + offset));

        // Assert
        assert_eq!(checks, [false, false, true, false, false, true]);
    }
}
