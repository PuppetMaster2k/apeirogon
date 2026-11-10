// SPDX-License-Identifier: GPL-3.0-or-later

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;
#[cfg(all(unix, not(target_os = "macos")))]
mod x11;

const GUI_NAME: &str = "a7n-gui";

/// Whether this session can identify focused full-screen windows.
#[must_use]
pub fn supported() -> bool {
    #[cfg(any(windows, target_os = "macos"))]
    {
        true
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        crate::input::detect_backend() == crate::input::Backend::X11
    }
    #[cfg(not(any(windows, unix)))]
    {
        false
    }
}

/// Whether the focused window is full-screen at this desktop point.
/// Unsupported sessions, query failures and our own GUI return false.
#[must_use]
pub fn focused_fullscreen_at(x: i32, y: i32) -> bool {
    if !supported() {
        return false;
    }
    platform_fullscreen_at(x, y).unwrap_or(false)
}

#[cfg(target_os = "macos")]
use macos::focused_fullscreen_at as platform_fullscreen_at;
#[cfg(windows)]
use windows::focused_fullscreen_at as platform_fullscreen_at;
#[cfg(all(unix, not(target_os = "macos")))]
use x11::focused_fullscreen_at as platform_fullscreen_at;
#[cfg(not(any(windows, unix)))]
fn platform_fullscreen_at(_: i32, _: i32) -> Option<bool> {
    None
}
