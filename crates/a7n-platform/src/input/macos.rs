// SPDX-License-Identifier: GPL-3.0-or-later

//! Session event tap and HID event injection.
//! shortcut: display bounds are sampled at start, restart after display changes.

mod capture;
mod inject;
mod keymap;

use std::io;

use objc2_core_foundation::CGPoint;
use objc2_core_graphics::{CGDisplayBounds, CGError, CGEvent, CGGetActiveDisplayList};
use tokio::sync::mpsc;

use super::{Bounds, Capture, Injector, InputError};
use crate::permissions;

const EVENT_CAPACITY: usize = 1024;
const CONTROL_CAPACITY: usize = 16;
const POINTER_CAPACITY: usize = 64;
const INJECTION_MARKER: i64 = 0x4137_4E49;
const MEDIA_EVENT: u32 = 14;
const MEDIA_SUBTYPE: i16 = 8;
const MEDIA_DOWN: u16 = 0xA;
const MEDIA_UP: u16 = 0xB;

pub(super) fn start() -> Result<(Capture, Injector), InputError> {
    if let Some(permission) = permissions::missing().first() {
        return Err(InputError::MissingPermission(permission.name()));
    }
    let bounds = display_bounds()?;
    let (events, events_rx) = mpsc::channel(EVENT_CAPACITY);
    let (control, control_rx) = mpsc::channel(CONTROL_CAPACITY);
    let (commands, commands_rx) = mpsc::channel(EVENT_CAPACITY);
    let (pointer, pointer_rx) = mpsc::channel(POINTER_CAPACITY);
    capture::spawn(events, control_rx)?;
    inject::spawn(bounds, commands_rx, pointer)?;
    Ok((
        Capture {
            bounds,
            events: events_rx,
            control,
        },
        Injector {
            bounds,
            commands,
            pointer: pointer_rx,
        },
    ))
}

fn check(error: CGError) -> io::Result<()> {
    if error == CGError::Success {
        Ok(())
    } else {
        Err(io::Error::other(format!("Core Graphics error {}", error.0)))
    }
}

fn pointer_position() -> io::Result<CGPoint> {
    let event = CGEvent::new(None).ok_or_else(|| io::Error::other("cannot read pointer"))?;
    Ok(CGEvent::location(Some(&event)))
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "display coordinates are finite and checked before conversion"
)]
fn display_bounds() -> io::Result<Bounds> {
    let mut count = 0;
    // SAFETY: A null output buffer with zero capacity queries the required count.
    check(unsafe { CGGetActiveDisplayList(0, std::ptr::null_mut(), &raw mut count) })?;
    let mut displays = vec![0; count as usize];
    // SAFETY: displays has count writable entries; count points to writable storage.
    check(unsafe { CGGetActiveDisplayList(count, displays.as_mut_ptr(), &raw mut count) })?;
    displays.truncate(count as usize);
    let mut edges = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for display in displays {
        let rect = CGDisplayBounds(display);
        edges.0 = edges.0.min(rect.origin.x);
        edges.1 = edges.1.min(rect.origin.y);
        edges.2 = edges.2.max(rect.origin.x + rect.size.width);
        edges.3 = edges.3.max(rect.origin.y + rect.size.height);
    }
    let (x, y, right, bottom) = edges;
    if ![x, y, right, bottom]
        .iter()
        .all(|v| v.is_finite() && *v >= f64::from(i32::MIN) && *v <= f64::from(i32::MAX))
        || right <= x
        || bottom <= y
    {
        return Err(io::Error::other("invalid active display bounds"));
    }
    Ok(Bounds {
        x: x as i32,
        y: y as i32,
        width: (right - x) as u32,
        height: (bottom - y) as u32,
    })
}
