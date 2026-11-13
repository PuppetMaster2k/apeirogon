// SPDX-License-Identifier: GPL-3.0-or-later

//! Ends the tray popup menu's modal state on Windows and repaints the app.
use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::Graphics::Gdi::{RDW_ALLCHILDREN, RDW_FRAME, RDW_INVALIDATE, RedrawWindow};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{EnumThreadWindows, PostMessageW, WM_NULL};

/// Posts `WM_NULL` to the windows of this thread, which include the hidden
/// `tray-icon` window, as the `TrackPopupMenu` remarks require after a
/// notification icon menu, and repaints them: the menu leaves the foreground
/// on the hidden window, and the app window otherwise keeps its old frame
/// until the mouse moves over it. Must run on the thread that owns the tray.
pub fn dismiss() {
    // SAFETY: the enumeration callback only uses the handles Windows passes it.
    unsafe {
        EnumThreadWindows(GetCurrentThreadId(), Some(dismiss_window), 0);
    }
}

unsafe extern "system" fn dismiss_window(hwnd: HWND, _: LPARAM) -> i32 {
    // SAFETY: `hwnd` is a live window handle supplied by `EnumThreadWindows`;
    // `PostMessageW` takes no pointers and `RedrawWindow` gets null rects.
    unsafe {
        PostMessageW(hwnd, WM_NULL, 0, 0);
        RedrawWindow(
            hwnd,
            std::ptr::null(),
            std::ptr::null_mut(),
            RDW_INVALIDATE | RDW_FRAME | RDW_ALLCHILDREN,
        );
    }
    1
}
