// SPDX-License-Identifier: GPL-3.0-or-later

use objc2_core_foundation::{CFBoolean, CFString};
use objc2_core_graphics::{CGDisplayHideCursor, CGDisplayShowCursor, CGMainDisplayID};

use super::super::check;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn _CGSDefaultConnection() -> i32;
    fn CGSSetConnectionProperty(
        connection: i32,
        target: i32,
        key: &CFString,
        value: &CFBoolean,
    ) -> i32;
}

#[derive(Default)]
pub(super) struct Cursor {
    hidden: bool,
    background_enabled: bool,
    failed: bool,
}

impl Cursor {
    pub(super) fn hidden(&self) -> bool {
        self.hidden
    }

    pub(super) fn set_hidden(&mut self, hidden: bool) {
        if self.hidden == hidden || (hidden && self.failed) {
            return;
        }
        if !hidden {
            if check(CGDisplayShowCursor(CGMainDisplayID())).is_ok() {
                self.hidden = false;
            } else {
                eprintln!("kind=cursor_restore_failed");
            }
            return;
        }
        if !self.background_enabled {
            self.background_enabled = enable_background_hiding();
        }
        if !self.background_enabled || check(CGDisplayHideCursor(CGMainDisplayID())).is_err() {
            self.failed = true;
            eprintln!("kind=cursor_hide_failed");
            return;
        }
        self.hidden = true;
    }
}

impl Drop for Cursor {
    fn drop(&mut self) {
        self.set_hidden(false);
    }
}

fn enable_background_hiding() -> bool {
    // Private API used deliberately: if denied, leave the cursor visible for this capture.
    let key = CFString::from_static_str("SetsCursorInBackground");
    // SAFETY: WindowServer supplies this process's connection; both CF values
    // remain valid throughout the synchronous property call and are only read.
    unsafe {
        let connection = _CGSDefaultConnection();
        CGSSetConnectionProperty(connection, connection, &key, CFBoolean::new(true)) == 0
    }
}
