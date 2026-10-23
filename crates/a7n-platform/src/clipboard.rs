// SPDX-License-Identifier: GPL-3.0-or-later

//! Channels shared by the daemon and future clipboard backend threads.

use a7n_core::clipboard::{Generation, Item, LocalChange};
use tokio::sync::{mpsc, oneshot};

/// Backends must suppress changes caused by their own `Set` commands so that
/// remote clipboard writes cannot create a copy/announce feedback loop.
#[derive(Debug)]
pub struct Clipboard {
    pub changes: mpsc::Receiver<LocalChange>,
    pub commands: mpsc::Sender<ClipboardCommand>,
    /// The OS asked to paste an item that was offered without data.
    pub pastes: mpsc::Receiver<PasteRequest>,
}

#[derive(Debug)]
pub enum ClipboardCommand {
    Set {
        generation: Generation,
        items: Vec<Item>,
        /// Offered now, fetched when something pastes them.
        lazy: Vec<LazyItem>,
    },
}

/// An item whose data stays on the origin until a paste requests it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LazyItem {
    pub mime: String,
    pub size: u64,
    /// Index in the origin's `ClipboardAnnounce.items`.
    pub item_index: u32,
}

/// Backends block the paste until `reply` answers; `None` means the data is
/// gone (newer generation, peer unreachable, integrity failure).
#[derive(Debug)]
pub struct PasteRequest {
    pub generation: Generation,
    pub item_index: u32,
    pub reply: oneshot::Sender<Option<Vec<u8>>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ClipboardError {
    #[error("clipboard is not supported on this platform")]
    Unsupported,
}

/// # Errors
/// Returns [`ClipboardError::Unsupported`] until OS backends are implemented.
pub fn start() -> Result<Clipboard, ClipboardError> {
    Err(ClipboardError::Unsupported)
}
