// SPDX-License-Identifier: GPL-3.0-or-later

use std::io;
use std::ptr::null_mut;
use std::thread;
use std::time::Duration;

use a7n_core::clipboard::{IMAGE_PNG, TEXT_HTML, TEXT_PLAIN, limit_for, validate_png};
use a7n_proto::limits::CLIPBOARD_TEXT_MAX;
use windows_sys::Win32::Foundation::{GlobalFree, HWND};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows_sys::Win32::System::Ole::{CF_DIB, CF_UNICODETEXT};
use windows_sys::core::w;

use crate::clipboard::cf_html;

pub(super) const RETRY_INTERVAL: Duration = Duration::from_millis(10);
const OPEN_ATTEMPTS: usize = 5;
// shortcut: bound the whole CF_HTML allocation like UTF-16 text; read only the
// fragment in-place if apps need larger HTML context around a small fragment.
pub(super) const MAX_OS_BYTES: usize = (CLIPBOARD_TEXT_MAX + 1) * size_of::<u16>();

pub(super) struct Formats {
    pub html: u32,
    pub png: u32,
    pub exclude: u32,
    pub history: u32,
    pub cloud: u32,
}

impl Formats {
    pub(super) fn new() -> io::Result<Self> {
        // SAFETY: All names are static NUL-terminated UTF-16 strings.
        let formats = unsafe {
            Self {
                png: RegisterClipboardFormatW(w!("PNG")),
                html: RegisterClipboardFormatW(w!("HTML Format")),
                exclude: RegisterClipboardFormatW(w!(
                    "ExcludeClipboardContentFromMonitorProcessing"
                )),
                history: RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory")),
                cloud: RegisterClipboardFormatW(w!("CanUploadToCloudClipboard")),
            }
        };
        if [
            formats.html,
            formats.png,
            formats.exclude,
            formats.history,
            formats.cloud,
        ]
        .contains(&0)
        {
            return Err(io::Error::last_os_error());
        }
        Ok(formats)
    }

    pub(super) fn format(&self, mime: &str) -> Option<u32> {
        match mime {
            IMAGE_PNG => Some(self.png),
            TEXT_PLAIN => Some(u32::from(CF_UNICODETEXT)),
            TEXT_HTML => Some(self.html),
            _ => None,
        }
    }

    pub(super) fn write(&self, format: u32, data: &[u8]) -> bool {
        if format == self.png || format == u32::from(CF_DIB) {
            if data.len() > limit_for(IMAGE_PNG) || validate_png(data).is_err() {
                return false;
            }
            return if format == self.png {
                write_global(format, data)
            } else {
                super::image::to_dib(data).is_some_and(|dib| write_global(format, &dib))
            };
        }
        if data.len() > limit_for(TEXT_PLAIN) {
            return false;
        }
        let Ok(text) = std::str::from_utf8(data) else {
            return false;
        };
        let bytes = if format == u32::from(CF_UNICODETEXT) {
            text.encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_ne_bytes)
                .collect()
        } else if format == self.html {
            cf_html::encode(text)
        } else {
            return false;
        };
        write_global(format, &bytes)
    }
}

pub(super) struct Open;

impl Open {
    pub(super) fn clipboard(hwnd: HWND) -> Option<Self> {
        for attempt in 0..OPEN_ATTEMPTS {
            // SAFETY: hwnd is the live clipboard window on this thread.
            if unsafe { OpenClipboard(hwnd) } != 0 {
                return Some(Self);
            }
            if attempt + 1 < OPEN_ATTEMPTS {
                thread::sleep(RETRY_INTERVAL);
            }
        }
        None
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: This guard represents one successful OpenClipboard on this thread.
        unsafe { CloseClipboard() };
    }
}

// Call only while the clipboard is open; copy before releasing its global handle.
pub(super) fn read_global(format: u32, maximum: usize) -> Option<Vec<u8>> {
    // SAFETY: Caller holds Open; format is a registered or predefined format.
    let handle = unsafe { GetClipboardData(format) };
    if handle.is_null() {
        return None;
    }
    // SAFETY: The returned handle is owned by the clipboard and remains valid while open.
    let size = unsafe { GlobalSize(handle) };
    if size == 0 || size > maximum {
        return None;
    }
    // SAFETY: Locks this valid global allocation for reading.
    let pointer = unsafe { GlobalLock(handle) };
    if pointer.is_null() {
        return None;
    }
    // SAFETY: GlobalLock exposes size readable bytes until GlobalUnlock below.
    let bytes = unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), size) }.to_vec();
    // SAFETY: Matches this function's successful GlobalLock.
    unsafe { GlobalUnlock(handle) };
    Some(bytes)
}

fn write_global(format: u32, bytes: &[u8]) -> bool {
    // SAFETY: Requests a movable allocation of the exact buffer size.
    let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) };
    if handle.is_null() {
        return false;
    }
    // SAFETY: handle is a newly allocated global memory block.
    let pointer = unsafe { GlobalLock(handle) };
    if pointer.is_null() {
        // SAFETY: Ownership has not been transferred; free this allocation once.
        unsafe { GlobalFree(handle) };
        return false;
    }
    // SAFETY: Allocation holds bytes.len() bytes, source is valid and disjoint.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast::<u8>(), bytes.len());
        GlobalUnlock(handle);
    }
    // SAFETY: Clipboard is open or this is WM_RENDERFORMAT; movable handle is unlocked.
    if unsafe { SetClipboardData(format, handle) }.is_null() {
        // SAFETY: Failed SetClipboardData leaves allocation ownership with us.
        unsafe { GlobalFree(handle) };
        return false;
    }
    true
}

pub(super) fn offer(format: u32) {
    // SAFETY: Clipboard is open and owned by our window; NULL requests delayed rendering.
    unsafe { SetClipboardData(format, null_mut()) };
}
