// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;
use std::io;

use objc2_core_foundation::{
    CFMachPort, CFRetained, CFRunLoop, CFRunLoopSource, kCFRunLoopDefaultMode,
};
use objc2_core_graphics::{
    CGEvent, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
};

use super::{MEDIA_EVENT, State, callback};

pub(super) struct Tap {
    port: CFRetained<CFMachPort>,
    source: CFRetained<CFRunLoopSource>,
    run_loop: CFRetained<CFRunLoop>,
    grabbed: bool,
}

impl Tap {
    pub(super) fn install(state: &RefCell<State>, grabbed: bool) -> io::Result<Self> {
        let run_loop =
            CFRunLoop::current().ok_or_else(|| io::Error::other("no capture run loop"))?;
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
        let port = unsafe {
            CGEvent::tap_create(
                CGEventTapLocation::SessionEventTap,
                CGEventTapPlacement::HeadInsertEventTap,
                if grabbed {
                    CGEventTapOptions::Default
                } else {
                    CGEventTapOptions::ListenOnly
                },
                mask,
                Some(callback),
                std::ptr::from_ref(state).cast_mut().cast(),
            )
        }
        .ok_or_else(|| io::Error::other("cannot create session event tap"))?;
        let Some(source) = CFMachPort::new_run_loop_source(None, Some(&port), 0) else {
            port.invalidate();
            return Err(io::Error::other("cannot create event tap run-loop source"));
        };
        // SAFETY: Core Foundation exports this immutable mode for the process lifetime.
        run_loop.add_source(Some(&source), unsafe { kCFRunLoopDefaultMode });
        CGEvent::tap_enable(&port, true);
        Ok(Self {
            port,
            source,
            run_loop,
            grabbed,
        })
    }

    pub(super) fn set_grab(&mut self, state: &RefCell<State>, grabbed: bool) -> io::Result<()> {
        if self.grabbed == grabbed {
            return Ok(());
        }
        let replacement = match Self::install(state, grabbed) {
            Ok(tap) => tap,
            Err(error) if grabbed => {
                eprintln!("kind=input_grab_failed reason={error}");
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        state.borrow_mut().grab(grabbed);
        if state.borrow().grabbed == grabbed {
            *self = replacement;
        }
        Ok(())
    }

    pub(super) fn enable(&self) {
        CGEvent::tap_enable(&self.port, true);
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        // SAFETY: Core Foundation exports this immutable mode for the process lifetime.
        self.run_loop
            .remove_source(Some(&self.source), unsafe { kCFRunLoopDefaultMode });
        self.source.invalidate();
        self.port.invalidate();
    }
}
