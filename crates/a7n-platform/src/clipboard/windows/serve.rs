// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_proto::limits::CLIPBOARD_TEXT_MAX;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::DataExchange::{
    EmptyClipboard, GetClipboardOwner, GetClipboardSequenceNumber,
};

use super::State;
use super::data::{Open, offer};
use crate::clipboard::ClipboardCommand;

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
                }
            }
        }
        for item in lazy {
            if item.size > CLIPBOARD_TEXT_MAX as u64 {
                continue;
            }
            if let Some(format) = self.formats.format(&item.mime) {
                if !eager.contains(&format)
                    && !self.lazy.iter().any(|(existing, _)| *existing == format)
                {
                    offer(format);
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
        let Some(bytes) = crate::clipboard::fetch::fetch(generation, item.item_index, &self.pastes)
        else {
            return;
        };
        if bytes.len() as u64 == item.size && self.formats.write(format, &bytes) {
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
        let formats: Vec<_> = self.lazy.iter().map(|(format, _)| *format).collect();
        for format in formats {
            self.render(format);
        }
    }
}
