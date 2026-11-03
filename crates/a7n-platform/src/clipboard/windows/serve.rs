// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_proto::limits::CLIPBOARD_FETCH_TIMEOUT;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::DataExchange::{
    EmptyClipboard, GetClipboardOwner, GetClipboardSequenceNumber,
};
use windows_sys::Win32::System::Ole::{CF_DIB, CF_HDROP, DROPEFFECT_COPY};

use super::State;
use super::data::{Open, offer, write_global};
use crate::clipboard::{ClipboardCommand, FILE_PASTE_TIMEOUT};

impl State {
    pub(super) fn set(&mut self, hwnd: HWND, command: ClipboardCommand) {
        let Some(open) = Open::clipboard(hwnd) else {
            return;
        };
        // SAFETY: Clipboard is open on this thread with hwnd as its new owner.
        if unsafe { EmptyClipboard() } == 0 {
            return;
        }
        self.lazy.clear();
        let ClipboardCommand::Set {
            generation,
            items,
            lazy,
        } = command;
        self.generation = Some(generation);
        let mut eager = Vec::new();
        for item in items {
            if let Some(format) = self.formats.format(&item.mime) {
                if self.formats.write(format, &item.data) {
                    eager.push(format);
                    if item.mime == a7n_core::clipboard::IMAGE_PNG
                        && self.formats.write(u32::from(CF_DIB), &item.data)
                    {
                        eager.push(u32::from(CF_DIB));
                    }
                }
            }
        }
        for item in lazy {
            if !item.within_limit() {
                continue;
            }
            if let Some(format) = self.formats.format(&item.mime) {
                if !eager.contains(&format)
                    && !self.lazy.iter().any(|(existing, _)| *existing == format)
                {
                    offer(format);
                    if format == u32::from(CF_HDROP) {
                        // Without it Explorer may move the files out of the receive directory.
                        write_global(self.formats.drop_effect, &DROPEFFECT_COPY.to_ne_bytes());
                    }
                    if item.mime == a7n_core::clipboard::IMAGE_PNG {
                        offer(u32::from(CF_DIB));
                        self.lazy.push((u32::from(CF_DIB), item.clone()));
                    }
                    self.lazy.push((format, item));
                }
            }
        }
        // SAFETY: Capture while still open so another app cannot race this query.
        self.sequence = Some(unsafe { GetClipboardSequenceNumber() });
        drop(open);
    }

    pub(super) fn render(&mut self, format: u32) {
        let Some(index) = self.lazy.iter().position(|(offered, _)| *offered == format) else {
            return;
        };
        let Some(generation) = self.generation else {
            return;
        };
        let item = &self.lazy[index].1;
        let timeout = if format == u32::from(CF_HDROP) {
            // The reply arrives only after the files are downloaded.
            FILE_PASTE_TIMEOUT
        } else {
            CLIPBOARD_FETCH_TIMEOUT
        };
        let Some(bytes) =
            crate::clipboard::fetch::fetch(generation, item.item_index, timeout, &self.pastes)
        else {
            return;
        };
        // The size of a file list is the files', not the uri-list's.
        let sized = format == u32::from(CF_HDROP) || bytes.len() as u64 == item.size;
        if sized && self.formats.write(format, &bytes) {
            self.lazy.swap_remove(index);
        }
    }

    pub(super) fn render_all(&mut self, hwnd: HWND) {
        let Some(_open) = Open::clipboard(hwnd) else {
            return;
        };
        // SAFETY: Clipboard is open; check ownership before rendering without emptying it.
        if unsafe { GetClipboardOwner() } != hwnd {
            return;
        }
        // Not the file list: WM_RENDERALLFORMATS runs as the owner exits, and a
        // download of unbounded length would hold the shutdown. The files stay
        // in the receive directory; only the paste is lost.
        let formats: Vec<_> = self
            .lazy
            .iter()
            .map(|(format, _)| *format)
            .filter(|format| *format != u32::from(CF_HDROP))
            .collect();
        for format in formats {
            self.render(format);
        }
    }
}
