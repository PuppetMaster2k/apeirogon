// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;
use std::ffi::c_void;
use std::io;
use std::ptr::{NonNull, null_mut};
use std::sync::mpsc as sync;
use std::thread;
use std::time::Instant;

use a7n_proto::v1::{
    Button, ButtonEvent, InputEvent, KeyEvent, ScrollEvent, ScrollUnit, input_event,
};
use objc2::rc::autoreleasepool;
use objc2_app_kit::NSEvent;
use objc2_core_foundation::{CFMachPort, CFRunLoop, CGPoint, kCFRunLoopDefaultMode};
use objc2_core_graphics::{
    CGAssociateMouseAndMouseCursorPosition, CGDisplayHideCursor, CGDisplayShowCursor, CGEvent,
    CGEventField, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventTapProxy,
    CGEventType, CGMainDisplayID,
};
use tokio::sync::mpsc::error::{TryRecvError, TrySendError};
use tokio::sync::mpsc::{Receiver, Sender};

use super::{
    INJECTION_MARKER, MEDIA_DOWN, MEDIA_EVENT, MEDIA_SUBTYPE, MEDIA_UP, check, keymap,
    pointer_position,
};
use crate::input::{CaptureControl, Captured, DeadMan, InputError};

const WAKE_SECONDS: f64 = 0.05;
const KEY_COUNT: usize = 128;

struct State {
    events: Sender<Captured>,
    deadman: DeadMan,
    grabbed: bool,
    queue_full: bool,
    disabled: bool,
    keys: [bool; KEY_COUNT],
    anchor: CGPoint,
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
                keys: [false; KEY_COUNT],
                anchor: CGPoint::new(0.0, 0.0),
            });
            let setup = install(&state);
            match setup {
                Ok(tap) => {
                    let outcome = run(&tap, &state, control, &ready);
                    state.borrow_mut().grab(false);
                    tap.invalidate();
                    if let Err(error) = outcome {
                        let _ = ready.send(Err(error));
                    }
                }
                Err(error) => {
                    let _ = ready.send(Err(error));
                }
            }
        })
        .map_err(InputError::Thread)?;
    result
        .recv()
        .map_err(|error| io::Error::other(error.to_string()))??;
    Ok(())
}

fn install(state: &RefCell<State>) -> io::Result<objc2_core_foundation::CFRetained<CFMachPort>> {
    let types = [
        CGEventType::LeftMouseDown,
        CGEventType::LeftMouseUp,
        CGEventType::RightMouseDown,
        CGEventType::RightMouseUp,
        CGEventType::MouseMoved,
        CGEventType::LeftMouseDragged,
        CGEventType::RightMouseDragged,
        CGEventType::KeyDown,
        CGEventType::KeyUp,
        CGEventType::FlagsChanged,
        CGEventType(MEDIA_EVENT),
        CGEventType::ScrollWheel,
        CGEventType::OtherMouseDown,
        CGEventType::OtherMouseUp,
        CGEventType::OtherMouseDragged,
    ];
    let mask = types.iter().fold(0, |mask, ty| mask | (1_u64 << ty.0));
    // SAFETY: state lives on this thread until the tap is invalidated. The callback
    // uses it only during run-loop dispatch, with no other reference in use.
    unsafe {
        CGEvent::tap_create(
            CGEventTapLocation::SessionEventTap,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default,
            mask,
            Some(callback),
            std::ptr::from_ref(state).cast_mut().cast(),
        )
    }
    .ok_or_else(|| io::Error::other("cannot create session event tap"))
}

fn run(
    tap: &CFMachPort,
    state: &RefCell<State>,
    mut control: Receiver<CaptureControl>,
    ready: &sync::Sender<io::Result<()>>,
) -> io::Result<()> {
    let source = CFMachPort::new_run_loop_source(None, Some(tap), 0)
        .ok_or_else(|| io::Error::other("cannot create event tap run-loop source"))?;
    let run_loop = CFRunLoop::current().ok_or_else(|| io::Error::other("no capture run loop"))?;
    // SAFETY: Core Foundation exports this immutable mode constant for the process lifetime.
    let mode = unsafe { kCFRunLoopDefaultMode };
    run_loop.add_source(Some(&source), mode);
    CGEvent::tap_enable(tap, true);
    if ready.send(Ok(())).is_err() {
        return Ok(());
    }
    while state.borrow_mut().tick(&mut control) {
        if std::mem::take(&mut state.borrow_mut().disabled) {
            CGEvent::tap_enable(tap, true);
        }
        // The callback may access state only during this synchronous dispatch.
        CFRunLoop::run_in_mode(mode, WAKE_SECONDS, false);
    }
    run_loop.remove_source(Some(&source), mode);
    source.invalidate();
    Ok(())
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
        self.grabbed = grab;
    }

    fn tick(&mut self, control: &mut Receiver<CaptureControl>) -> bool {
        let now = Instant::now();
        loop {
            match control.try_recv() {
                Ok(CaptureControl::Keepalive) => self.deadman.keepalive(now),
                Ok(CaptureControl::Grab(grab)) => {
                    if grab {
                        self.deadman.keepalive(now);
                    }
                    self.grab(grab);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return false,
            }
        }
        if self.grabbed && self.deadman.expired(now) {
            self.grab(false);
            eprintln!("kind=input_deadman_ungrab");
        }
        if std::mem::take(&mut self.queue_full) {
            eprintln!("kind=capture_queue_full");
        }
        !self.events.is_closed()
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
                if *held == down {
                    return self.grabbed;
                }
                *held = down;
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
            CGEventType::ScrollWheel => {
                let continuous = field(CGEventField::ScrollWheelEventIsContinuous) != 0;
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
                Some(input_event::Event::Scroll(ScrollEvent {
                    dx: scroll_delta(field(x)),
                    dy: -scroll_delta(field(y)),
                    unit: unit.into(),
                }))
            }
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
            _ if ty.0 == MEDIA_EVENT => autoreleasepool(|_| media(event)),
            _ => None,
        };
        if let Some(event) = translated {
            self.emit(Captured::Input(InputEvent { event: Some(event) }));
        }
        self.grabbed
    }
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
    if swallow { null_mut() } else { event.as_ptr() }
}
