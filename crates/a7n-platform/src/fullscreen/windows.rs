// SPDX-License-Identifier: GPL-3.0-or-later

use std::mem::size_of;

use windows_sys::Win32::Foundation::{CloseHandle, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromPoint,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId, IsZoomed,
};

const SHELL_CLASSES: &[&str] = &["Progman", "WorkerW", "Shell_TrayWnd"];

const CLASS_NAME_CAPACITY: usize = 256;
const PROCESS_PATH_CAPACITY: usize = 32768;

pub(super) fn focused_fullscreen_at(x: i32, y: i32) -> Option<bool> {
    // SAFETY: read-only foreground query has no preconditions.
    let window = unsafe { GetForegroundWindow() };
    if window.is_null() {
        return None;
    }
    let mut class = [0; CLASS_NAME_CAPACITY];
    // SAFETY: window is live and class points to a writable buffer of the supplied size.
    let length =
        unsafe { GetClassNameW(window, class.as_mut_ptr(), i32::try_from(class.len()).ok()?) };
    let class = String::from_utf16(&class[..usize::try_from(length).ok()?]).ok()?;
    if class.is_empty() {
        return None;
    }
    if SHELL_CLASSES.contains(&class.as_str()) {
        return Some(false);
    }
    let mut pid = 0;
    // SAFETY: pid is a writable output for the live foreground window.
    unsafe { GetWindowThreadProcessId(window, &raw mut pid) };
    let mut path = vec![0; PROCESS_PATH_CAPACITY];
    let mut length = u32::try_from(path.len()).ok()?;
    // SAFETY: requesting query-only access to the window's owning process.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }
    // SAFETY: process is live and path/length describe a writable buffer.
    let queried =
        unsafe { QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &raw mut length) };
    // SAFETY: process was opened above and is closed exactly once after its last use.
    unsafe { CloseHandle(process) };
    if queried == 0 {
        return None;
    }
    let path = String::from_utf16(&path[..usize::try_from(length).ok()?]).ok()?;
    if path
        .rsplit('\\')
        .next()?
        .eq_ignore_ascii_case(&format!("{}.exe", super::GUI_NAME))
    {
        return Some(false);
    }
    let mut rect = RECT::default();
    let mut monitor_info = MONITORINFO {
        cbSize: u32::try_from(size_of::<MONITORINFO>()).ok()?,
        ..Default::default()
    };
    // SAFETY: window is live; monitor is queried for the supplied point, output buffers are valid.
    unsafe {
        let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONULL);
        if monitor.is_null()
            || GetMonitorInfoW(monitor, &raw mut monitor_info) == 0
            || GetWindowRect(window, &raw mut rect) == 0
        {
            return None;
        }
    }
    // SAFETY: read-only maximized-state query for the foreground window.
    if unsafe { IsZoomed(window) } != 0 {
        return Some(false);
    }
    let display = monitor_info.rcMonitor;
    Some(
        rect.left <= display.left
            && rect.top <= display.top
            && rect.right >= display.right
            && rect.bottom >= display.bottom,
    )
}
