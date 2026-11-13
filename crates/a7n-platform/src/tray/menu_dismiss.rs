// SPDX-License-Identifier: GPL-3.0-or-later

//! Ends the tray popup menu's modal state on Windows and repaints the app.
use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::Graphics::Gdi::{RDW_ALLCHILDREN, RDW_FRAME, RDW_INVALIDATE, RedrawWindow};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumThreadWindows, FindWindowW, PostMessageW, WM_NULL,
};

/// Window class `tray-icon` registers for its hidden message window.
const TRAY_WINDOW_CLASS: &[u16] = &[
    b't' as u16,
    b'r' as u16,
    b'a' as u16,
    b'y' as u16,
    b'_' as u16,
    b'i' as u16,
    b'c' as u16,
    b'o' as u16,
    b'n' as u16,
    b'_' as u16,
    b'a' as u16,
    b'p' as u16,
    b'p' as u16,
    0,
];

/// Posts `WM_NULL` to the hidden tray window, as the `TrackPopupMenu` remarks
/// require after a notification icon menu, and repaints the other windows of
/// this thread: the menu leaves the foreground on the hidden window, and the
/// app window otherwise keeps its old frame until the mouse moves over it.
pub fn dismiss() {
    // SAFETY: the class name is NUL-terminated and a null title matches any
    // title; the enumeration callback only calls `RedrawWindow` on the
    // handles Windows passes it, and `PostMessageW` takes no pointers.
    unsafe {
        let hwnd = FindWindowW(TRAY_WINDOW_CLASS.as_ptr(), std::ptr::null());
        if !hwnd.is_null() {
            PostMessageW(hwnd, WM_NULL, 0, 0);
        }
        EnumThreadWindows(GetCurrentThreadId(), Some(redraw), 0);
    }
}

unsafe extern "system" fn redraw(hwnd: HWND, _: LPARAM) -> i32 {
    // SAFETY: `hwnd` is a live window handle supplied by `EnumThreadWindows`.
    unsafe {
        RedrawWindow(
            hwnd,
            std::ptr::null(),
            std::ptr::null_mut(),
            RDW_INVALIDATE | RDW_FRAME | RDW_ALLCHILDREN,
        );
    }
    1
}
