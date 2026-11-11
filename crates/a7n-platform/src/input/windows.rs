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
    let bounds = virtual_bounds()?;
    let monitors: std::sync::Arc<[Bounds]> =
        monitor_bounds().unwrap_or_else(|| vec![bounds]).into();
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
            monitors: monitors.clone(),
            events: events_rx,
            control: control_tx,
        },
        Injector {
            bounds,
            monitors,
            commands: commands_tx,
            pointer: pointer_rx,
        },
    ))
}

pub(crate) fn virtual_bounds() -> io::Result<Bounds> {
    // SAFETY: Sets process-wide coordinate policy; no pointers are passed.
    // Failure is harmless when awareness was already set by a manifest/host.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };

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

fn monitor_bounds() -> Option<Vec<Bounds>> {
    use windows_sys::Win32::Graphics::Gdi::EnumDisplayMonitors;
    let mut monitors = Vec::new();
    // SAFETY: callback receives a live Vec pointer for the duration of this synchronous call.
    let ok = unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect_monitor),
            (&raw mut monitors) as isize,
        )
    };
    (ok != 0 && !monitors.is_empty()).then_some(monitors)
}

unsafe extern "system" fn collect_monitor(
    _monitor: windows_sys::Win32::Graphics::Gdi::HMONITOR,
    _dc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: *mut windows_sys::Win32::Foundation::RECT,
    data: isize,
) -> i32 {
    // SAFETY: EnumDisplayMonitors supplies a valid RECT and data is our live Vec pointer.
    let (rect, monitors) = unsafe { (&*rect, &mut *(data as *mut Vec<Bounds>)) };
    if let (Ok(width), Ok(height)) = (
        u32::try_from(i64::from(rect.right) - i64::from(rect.left)),
        u32::try_from(i64::from(rect.bottom) - i64::from(rect.top)),
    ) {
        if width > 0 && height > 0 {
            monitors.push(Bounds {
                x: rect.left,
                y: rect.top,
                width,
                height,
            });
        }
    }
    1
}
