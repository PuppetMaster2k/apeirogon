// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_core::clipboard::{IMAGE_PNG, Item, LocalChange, TEXT_HTML, TEXT_PLAIN, limit_for};
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{CloseHandle, HWND};

use a7n_proto::limits::MANIFEST_ENTRIES_MAX;
use windows_sys::Win32::System::DataExchange::{
    GetClipboardData, GetClipboardOwner, GetClipboardSequenceNumber, IsClipboardFormatAvailable,
};
use windows_sys::Win32::System::Ole::{CF_HDROP, CF_UNICODETEXT};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

use super::State;
use super::data::{MAX_OS_BYTES, Open, read_global};
use crate::clipboard::cf_html;

const PROCESS_PATH_CAPACITY: usize = 32768;

impl State {
    pub(super) fn changed(&mut self, hwnd: HWND) {
        let Some(open) = Open::clipboard(hwnd) else {
            return;
        };
        // SAFETY: Clipboard metadata queries require no pointers; clipboard is open.
        let (sequence, owner) = unsafe { (GetClipboardSequenceNumber(), GetClipboardOwner()) };
        if owner == hwnd || self.sequence == Some(sequence) {
            return;
        }
        let sensitive = available(self.formats.exclude)
            || [self.formats.history, self.formats.cloud]
                .into_iter()
                .any(|format| {
                    available(format)
                        && read_global(format, MAX_OS_BYTES).is_none_or(|bytes| {
                            // Unreadable privacy markers fail closed.
                            bytes
                                .get(..size_of::<u32>())
                                .is_none_or(|value| value == 0_u32.to_ne_bytes())
                        })
                });
        let mut change = LocalChange {
            files: Vec::new(),
            items: Vec::new(),
            sensitive,
            source_app: source_app(owner),
        };
        if !sensitive {
            let image = if available(self.formats.png) {
                read_global(self.formats.png, limit_for(IMAGE_PNG))
                    .filter(|data| a7n_core::clipboard::validate_png(data).is_ok())
            } else {
                use windows_sys::Win32::System::Ole::{CF_DIB, CF_DIBV5};
                [CF_DIBV5, CF_DIB].into_iter().find_map(|format| {
                    read_global(u32::from(format), super::image::MAX_DIB_BYTES)
                        .and_then(|bytes| super::image::from_dib(&bytes))
                })
            };
            if let Some(data) = image {
                change.items.push(Item {
                    mime: IMAGE_PNG.into(),
                    data,
                });
            }
            change.files = dropped_files();
            if let Some(data) = plain_text() {
                change.items.push(Item {
                    mime: TEXT_PLAIN.into(),
                    data,
                });
            }
            if let Some(bytes) = read_global(self.formats.html, MAX_OS_BYTES) {
                if let Some(fragment) =
                    cf_html::decode(&bytes).filter(|data| data.len() <= limit_for(TEXT_HTML))
                {
                    change.items.push(Item {
                        mime: TEXT_HTML.into(),
                        data: fragment.to_vec(),
                    });
                }
            }
        }
        // Delayed rendering can advance the sequence while ownership stays stable.
        // SAFETY: Query requires no pointers and clipboard remains open here.
        let unchanged = unsafe { GetClipboardOwner() == owner };
        drop(open);
        if unchanged {
            let _ = self.changes.blocking_send(change);
        }
    }
}

fn available(format: u32) -> bool {
    // SAFETY: Format is predefined or registered for this process.
    unsafe { IsClipboardFormatAvailable(format) != 0 }
}

/// `DragQueryFileW` asks for the file count with this index.
const FILE_COUNT_INDEX: u32 = u32::MAX;

/// Paths of a copied file list; call only while the clipboard is open.
fn dropped_files() -> Vec<PathBuf> {
    if !available(u32::from(CF_HDROP)) {
        return Vec::new();
    }
    // SAFETY: The clipboard is open; the handle stays owned by the clipboard.
    let drop = unsafe { GetClipboardData(u32::from(CF_HDROP)) };
    if drop.is_null() {
        return Vec::new();
    }
    // SAFETY: drop is a live HDROP; a null buffer only asks for the count.
    let count = unsafe { DragQueryFileW(drop, FILE_COUNT_INDEX, null_mut(), 0) };
    let Ok(count) = usize::try_from(count) else {
        return Vec::new();
    };
    // More entries than a manifest may hold can never be sent; read none.
    if count > MANIFEST_ENTRIES_MAX {
        return Vec::new();
    }
    let mut paths = Vec::with_capacity(count);
    for index in (0..count).filter_map(|index| u32::try_from(index).ok()) {
        let Some(path) = dropped_file(drop, index) else {
            return Vec::new();
        };
        paths.push(path);
    }
    paths
}

fn dropped_file(drop: HDROP, index: u32) -> Option<PathBuf> {
    // SAFETY: drop is a live HDROP; a null buffer only asks for the length.
    let length = unsafe { DragQueryFileW(drop, index, null_mut(), 0) };
    let length = usize::try_from(length).ok()?;
    if length == 0 || length >= PROCESS_PATH_CAPACITY {
        return None;
    }
    let mut buffer = vec![0_u16; length + 1];
    let capacity = u32::try_from(buffer.len()).ok()?;
    // SAFETY: buffer has capacity writable UTF-16 slots, counting the terminator.
    let copied = unsafe { DragQueryFileW(drop, index, buffer.as_mut_ptr(), capacity) };
    let copied = usize::try_from(copied)
        .ok()
        .filter(|copied| *copied == length)?;
    Some(PathBuf::from(OsString::from_wide(&buffer[..copied])))
}

fn plain_text() -> Option<Vec<u8>> {
    let bytes = read_global(u32::from(CF_UNICODETEXT), MAX_OS_BYTES)?;
    let mut units = Vec::new();
    let mut terminated = false;
    for pair in bytes.chunks_exact(size_of::<u16>()) {
        let unit = u16::from_ne_bytes([pair[0], pair[1]]);
        if unit == 0 {
            terminated = true;
            break;
        }
        units.push(unit);
    }
    if !terminated {
        return None;
    }
    let text = String::from_utf16(&units).ok()?;
    (text.len() <= limit_for(TEXT_PLAIN)).then(|| text.into_bytes())
}

fn source_app(owner: HWND) -> Option<String> {
    if owner.is_null() {
        return None;
    }
    let mut pid = 0;
    // SAFETY: owner is an OS-provided HWND; pid is valid writable storage.
    unsafe { GetWindowThreadProcessId(owner, &raw mut pid) };
    let mut path = vec![0_u16; PROCESS_PATH_CAPACITY];
    let mut length = u32::try_from(path.len()).ok()?;
    // SAFETY: Requests query-only access to the clipboard owner's process.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }
    // SAFETY: process is open, path has length writable UTF-16 slots; length describes them.
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            path.as_mut_ptr(),
            &raw mut length,
        )
    };
    // SAFETY: Close this function's successful OpenProcess exactly once.
    unsafe { CloseHandle(process) };
    if result == 0 {
        return None;
    }
    let path = String::from_utf16(path.get(..usize::try_from(length).ok()?)?).ok()?;
    path.rsplit(['\\', '/'])
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}
