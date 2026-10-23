// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading a foreign selection and answering pastes of ours, for both backends.

use std::os::fd::OwnedFd;
use std::sync::Arc;

use a7n_core::clipboard::Item;
use a7n_proto::limits::CLIPBOARD_TEXT_MAX;
use tokio::sync::mpsc::Sender;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, oneshot};
use tokio::time::timeout;

use super::served::{Serve, Served};
use super::{MAX_TRANSFERS, PASTE_TIMEOUT, READ_TIMEOUT, WRITE_TIMEOUT, pipe};
use crate::clipboard::PasteRequest;

/// Caps the pastes served at once.
#[derive(Debug, Clone)]
pub(super) struct Limiter(Arc<Semaphore>);

impl Limiter {
    pub(super) fn new() -> Self {
        Self(Arc::new(Semaphore::new(MAX_TRANSFERS)))
    }

    pub(super) fn acquire(&self) -> Option<OwnedSemaphorePermit> {
        Arc::clone(&self.0).try_acquire_owned().ok()
    }
}

/// Reads each format within the text limit and the read timeout. A format
/// that fails is left out, like in the X11 backend.
pub(super) async fn read_items(reads: Vec<(&'static str, OwnedFd)>) -> Vec<Item> {
    let mut items = Vec::new();
    for (mime, fd) in reads {
        let read = timeout(READ_TIMEOUT, pipe::read_limited(fd, CLIPBOARD_TEXT_MAX)).await;
        if let Ok(Ok(Some(data))) = read {
            if !data.is_empty() {
                items.push(Item {
                    mime: mime.to_owned(),
                    data,
                });
            }
        }
    }
    items
}

/// Answers one paste: inline data directly, lazy data after the origin
/// replied. Returns whether the reader got everything.
pub(super) async fn answer(
    served: &Served,
    requested: &str,
    pastes: &Sender<PasteRequest>,
    fd: OwnedFd,
) -> bool {
    let fetched;
    let data = match served.decide(requested) {
        Serve::Inline(data) => data,
        Serve::Lazy {
            generation,
            item_index,
        } => {
            let (reply, receiver) = oneshot::channel();
            let request = PasteRequest {
                generation,
                item_index,
                reply,
            };
            if pastes.try_send(request).is_err() {
                return false;
            }
            fetched = timeout(PASTE_TIMEOUT, receiver).await;
            match &fetched {
                Ok(Ok(Some(data))) if data.len() <= CLIPBOARD_TEXT_MAX => data,
                _ => return false,
            }
        }
        Serve::Refuse => return false,
    };
    matches!(
        timeout(WRITE_TIMEOUT, pipe::write_all(fd, data)).await,
        Ok(Ok(()))
    )
}
