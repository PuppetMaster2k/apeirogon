// SPDX-License-Identifier: GPL-3.0-or-later

use std::io;
use std::ptr::null_mut;

use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateCursor, DestroyCursor, OCR_APPSTARTING, OCR_CROSS, OCR_HAND, OCR_HELP, OCR_IBEAM, OCR_NO,
    OCR_NORMAL, OCR_SIZEALL, OCR_SIZENESW, OCR_SIZENS, OCR_SIZENWSE, OCR_SIZEWE, OCR_UP, OCR_WAIT,
    SPI_SETCURSORS, SetSystemCursor, SystemParametersInfoW,
};

const CURSOR_SIDE: i32 = 32;
const MASK_BYTES: usize = 128;
const SYSTEM_CURSORS: &[u32] = &[
    OCR_NORMAL,
    OCR_IBEAM,
    OCR_WAIT,
    OCR_CROSS,
    OCR_UP,
    OCR_SIZENWSE,
    OCR_SIZENESW,
    OCR_SIZEWE,
    OCR_SIZENS,
    OCR_SIZEALL,
    OCR_NO,
    OCR_HAND,
    OCR_APPSTARTING,
    OCR_HELP,
];

#[derive(Default)]
pub(super) struct Cursor {
    hidden: bool,
}

impl Cursor {
    pub(super) fn new() -> Self {
        // System cursor replacements survive a crashed process.
        let hidden = match restore() {
            Ok(()) => false,
            Err(error) => {
                eprintln!("kind=cursor_restore_failed error={error}");
                true
            }
        };
        Self { hidden }
    }

    pub(super) fn hidden(&self) -> bool {
        self.hidden
    }

    pub(super) fn set_hidden(&mut self, hidden: bool) {
        if hidden == self.hidden {
            return;
        }
        // Keep restoration armed after a partial replacement or failed restore.
        self.hidden = true;
        let result = if hidden { hide() } else { restore() };
        match result {
            Ok(()) => self.hidden = hidden,
            Err(error) => {
                eprintln!("kind=cursor_visibility_failed error={error}");
                if restore().is_ok() {
                    self.hidden = false;
                }
            }
        }
    }
}

impl Drop for Cursor {
    fn drop(&mut self) {
        if self.hidden {
            self.set_hidden(false);
        }
    }
}

fn restore() -> io::Result<()> {
    // SAFETY: SPI_SETCURSORS takes no pointer and reloads the user's cursor scheme.
    if unsafe { SystemParametersInfoW(SPI_SETCURSORS, 0, null_mut(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn hide() -> io::Result<()> {
    let and_mask = [u8::MAX; MASK_BYTES];
    let xor_mask = [0u8; MASK_BYTES];
    for &id in SYSTEM_CURSORS {
        // SAFETY: Both masks contain 32 rows of 32 bits; AND=1/XOR=0 is transparent.
        let cursor = unsafe {
            CreateCursor(
                null_mut(),
                0,
                0,
                CURSOR_SIDE,
                CURSOR_SIDE,
                and_mask.as_ptr().cast(),
                xor_mask.as_ptr().cast(),
            )
        };
        if cursor.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: Each call receives a fresh, non-shared cursor and a documented id.
        // On success Windows consumes the cursor, so it must not be reused.
        if unsafe { SetSystemCursor(cursor, id) } == 0 {
            let error = io::Error::last_os_error();
            // SAFETY: Replacement failed; the fresh cursor remains ours.
            unsafe { DestroyCursor(cursor) };
            return Err(error);
        }
    }
    Ok(())
}
