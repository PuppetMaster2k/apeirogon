// SPDX-License-Identifier: GPL-3.0-or-later

//! Windows low-level hook capture and scan-code `SendInput` injection.
//!
//! Known limitation: virtual screen bounds are sampled at start; display
//! configuration changes are not followed.

mod capture;
mod inject;
mod keymap;
mod mouse;

use std::io;

use tokio::sync::mpsc;
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN,
};

use super::{Bounds, Capture, Injector, InputError};

const CAPTURE_EVENT_CAPACITY: usize = 1024;
const CAPTURE_CONTROL_CAPACITY: usize = 16;
const INJECT_COMMAND_CAPACITY: usize = 1024;
const INJECT_POINTER_CAPACITY: usize = 64;
/// Fits both 32-bit and 64-bit `ULONG_PTR`.
const INJECTION_MARKER: usize = 0x4137_4E49;

pub(super) fn start() -> Result<(Capture, Injector), InputError> {
    // SAFETY: Sets process-wide coordinate policy; no pointers are passed.
    // Failure is harmless when awareness was already set by a manifest/host.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let bounds = virtual_bounds()?;
    let (events_tx, events_rx) = mpsc::channel(CAPTURE_EVENT_CAPACITY);
    let (control_tx, control_rx) = mpsc::channel(CAPTURE_CONTROL_CAPACITY);
    let (commands_tx, commands_rx) = mpsc::channel(INJECT_COMMAND_CAPACITY);
    let (pointer_tx, pointer_rx) = mpsc::channel(INJECT_POINTER_CAPACITY);

    // If injection cannot start, dropping control_tx lets capture unhook.
    capture::spawn(events_tx, control_rx)?;
    inject::spawn(bounds, commands_rx, pointer_tx)?;
    Ok((
        Capture {
            bounds,
            events: events_rx,
            control: control_tx,
        },
        Injector {
            bounds,
            commands: commands_tx,
            pointer: pointer_rx,
        },
    ))
}

fn virtual_bounds() -> io::Result<Bounds> {
    // SAFETY: These metric queries take no pointers and have no preconditions.
    let (x, y, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    if width <= 0 || height <= 0 {
        return Err(io::Error::other("virtual screen is empty"));
    }
    Ok(Bounds {
        x,
        y,
        width: width.unsigned_abs(),
        height: height.unsigned_abs(),
    })
}

fn pointer_position() -> io::Result<POINT> {
    let mut point = POINT::default();
    // SAFETY: point is valid writable storage for one POINT.
    if unsafe { GetCursorPos(&raw mut point) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(point)
}
