// SPDX-License-Identifier: GPL-3.0-or-later

//! Text clipboard ordering, exclusion and validation without platform I/O.

use a7n_proto::v1::{ClipboardAnnounce, ClipboardItem};
use a7n_proto::{MachineId, limits};
use sha2::{Digest, Sha256};

pub const TEXT_PLAIN: &str = "text/plain;charset=utf-8";
pub const TEXT_HTML: &str = "text/html;charset=utf-8";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Generation {
    pub generation: u64,
    pub origin: MachineId,
}

#[derive(Debug, Default)]
pub struct Clock {
    seen: u64,
}

impl Clock {
    pub fn observe(&mut self, generation: &Generation) {
        self.seen = self.seen.max(generation.generation);
    }

    pub fn next(&mut self, own: MachineId) -> Generation {
        // ponytail: exhaustion stops advancing; widen the wire counter if it ever becomes reachable.
        self.seen = self.seen.saturating_add(1);
        Generation {
            generation: self.seen,
            origin: own,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub mime: String,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub struct LocalChange {
    pub items: Vec<Item>,
    pub sensitive: bool,
    pub source_app: Option<String>,
}

#[derive(Debug)]
pub struct Exclusion;

impl Exclusion {
    #[must_use]
    pub fn allows(change: &LocalChange, deny_apps: &[String]) -> bool {
        !change.sensitive
            && !change.source_app.as_ref().is_some_and(|app| {
                deny_apps
                    .iter()
                    .any(|denied| denied.eq_ignore_ascii_case(app))
            })
    }
}

fn supported(mime: &str) -> bool {
    matches!(mime, TEXT_PLAIN | TEXT_HTML)
}

#[must_use]
pub fn announce(items: Vec<Item>, generation: Generation) -> Option<ClipboardAnnounce> {
    let mut inline = 0;
    let mut wire = Vec::new();
    for item in items
        .into_iter()
        .filter(|item| supported(&item.mime) && !item.data.is_empty())
    {
        if item.data.len() > limits::CLIPBOARD_TEXT_MAX {
            return None;
        }
        let size = item.data.len();
        let sha256 = Sha256::digest(&item.data).to_vec();
        let inline_data = if inline + size <= limits::CLIPBOARD_INLINE_MAX {
            inline += size;
            item.data
        } else {
            Vec::new()
        };
        wire.push(ClipboardItem {
            mime: item.mime,
            size: size as u64,
            sha256,
            inline_data,
        });
    }
    (!wire.is_empty()).then_some(ClipboardAnnounce {
        origin: generation.origin.as_bytes().to_vec(),
        generation: generation.generation,
        items: wire,
        files: None,
    })
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ClipboardError {
    #[error("clipboard origin must be 16 bytes")]
    Origin,
    #[error("clipboard item exceeds the text limit")]
    Size,
    #[error("clipboard inline total exceeds the limit")]
    InlineLimit,
    #[error("clipboard inline length does not match size")]
    Length,
    #[error("clipboard digest is invalid")]
    Digest,
}

#[derive(Debug)]
pub struct Received {
    pub generation: Generation,
    pub items: Vec<Item>,
    pub pending: Vec<u32>,
}

/// Validate before applying any data or advancing the current clipboard.
/// # Errors
/// Rejects invalid origins, lengths, digests and clipboard limits.
pub fn accept(
    current: Option<Generation>,
    incoming: &ClipboardAnnounce,
) -> Result<Option<Received>, ClipboardError> {
    let generation = Generation {
        generation: incoming.generation,
        origin: MachineId::try_from(incoming.origin.as_slice())
            .map_err(|_| ClipboardError::Origin)?,
    };
    let mut inline = 0;
    let mut items = Vec::new();
    let mut pending = Vec::new();
    for (index, item) in incoming.items.iter().enumerate() {
        if item.size > limits::CLIPBOARD_TEXT_MAX as u64 {
            return Err(ClipboardError::Size);
        }
        if item.sha256.len() != Sha256::output_size() {
            return Err(ClipboardError::Digest);
        }
        inline += item.inline_data.len();
        if inline > limits::CLIPBOARD_INLINE_MAX {
            return Err(ClipboardError::InlineLimit);
        }
        if !item.inline_data.is_empty() || item.size == 0 {
            if item.inline_data.len() as u64 != item.size {
                return Err(ClipboardError::Length);
            }
            if Sha256::digest(&item.inline_data).as_slice() != item.sha256 {
                return Err(ClipboardError::Digest);
            }
            if supported(&item.mime) && item.size != 0 {
                items.push(Item {
                    mime: item.mime.clone(),
                    data: item.inline_data.clone(),
                });
            }
        } else if supported(&item.mime) {
            pending.push(u32::try_from(index).map_err(|_| ClipboardError::Size)?);
        }
    }
    if current.is_some_and(|current| generation <= current) {
        return Ok(None);
    }
    Ok(Some(Received {
        generation,
        items,
        pending,
    }))
}

#[cfg(test)]
mod tests;
