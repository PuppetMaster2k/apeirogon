// SPDX-License-Identifier: GPL-3.0-or-later

//! Channels shared by the daemon and future clipboard backend threads.

use a7n_core::clipboard::{Generation, Item, LocalChange};
use tokio::sync::mpsc;

/// Backends must suppress changes caused by their own `Set` commands so that
/// remote clipboard writes cannot create a copy/announce feedback loop.
#[derive(Debug)]
pub struct Clipboard {
    pub changes: mpsc::Receiver<LocalChange>,
    pub commands: mpsc::Sender<ClipboardCommand>,
}

#[derive(Debug)]
pub enum ClipboardCommand {
    Set {
        generation: Generation,
        items: Vec<Item>,
    },
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
