// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading a foreign selection and answering pastes of ours, for both backends.

use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use a7n_core::clipboard::{Item, LocalChange};
use a7n_proto::limits::CLIPBOARD_FETCH_TIMEOUT;
#[cfg(test)]
use a7n_proto::limits::CLIPBOARD_TEXT_MAX;
use futures_util::future::join_all;
use tokio::sync::mpsc::Sender;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, oneshot};
use tokio::time::timeout;

use super::served::{Serve, Served};
use super::{MAX_TRANSFERS, READ_TIMEOUT, WRITE_TIMEOUT, pipe};
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
async fn read_items(reads: Vec<(&'static str, OwnedFd)>) -> Vec<Item> {
    join_all(reads.into_iter().map(|(mime, fd)| async move {
        let read = timeout(
            READ_TIMEOUT,
            pipe::read_limited(fd, a7n_core::clipboard::limit_for(mime)),
        )
        .await;
        match read {
            Ok(Ok(Some(data))) if !data.is_empty() => Some(Item {
                mime: mime.to_owned(),
                data,
            }),
            _ => None,
        }
    }))
    .await
    .into_iter()
    .flatten()
    .collect()
}

pub(super) async fn publish(
    reads: Vec<(&'static str, OwnedFd)>,
    sensitive: bool,
    epoch: &AtomicU64,
    stamp: u64,
    changes: &Sender<LocalChange>,
) {
    let items = read_items(reads).await;
    if epoch.load(Ordering::SeqCst) != stamp {
        return;
    }
    let change = LocalChange {
        items,
        sensitive,
        source_app: None,
    };
    let _ = changes.send(change).await;
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
            fetched = timeout(CLIPBOARD_FETCH_TIMEOUT, receiver).await;
            match &fetched {
                Ok(Ok(Some(data)))
                    if data.len()
                        <= a7n_core::clipboard::limit_for(
                            crate::clipboard::formats::item_mime_of(requested).unwrap_or(""),
                        ) =>
                {
                    data
                }
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

#[cfg(test)]
mod tests {
    use rustix::pipe::{PipeFlags, pipe_with};

    use super::*;

    #[tokio::test]
    async fn read_items_interdependent_writers_reads_concurrently_in_input_order() {
        // Arrange
        let (first, first_writer) = pipe_with(PipeFlags::CLOEXEC).unwrap();
        let (second, second_writer) = pipe_with(PipeFlags::CLOEXEC).unwrap();
        let data = vec![7; CLIPBOARD_TEXT_MAX];
        // The second pipe must be drained before the first writer can close.
        let write = async {
            pipe::write_all(second_writer, &data).await.unwrap();
            pipe::write_all(first_writer, b"first").await.unwrap();
        };

        // Act
        let (items, ()) = tokio::join!(
            read_items(vec![("text/plain", first), ("text/html", second)]),
            write,
        );

        // Assert
        assert_eq!(
            items,
            vec![
                Item {
                    mime: "text/plain".to_owned(),
                    data: b"first".to_vec()
                },
                Item {
                    mime: "text/html".to_owned(),
                    data
                },
            ]
        );
    }

    #[tokio::test]
    async fn publish_selection_changes_during_read_discards_stale_items() {
        // Arrange
        let (reader, writer) = pipe_with(PipeFlags::CLOEXEC).unwrap();
        let epoch = AtomicU64::new(1);
        let (changes, mut receiver) = tokio::sync::mpsc::channel(1);
        let write = async {
            epoch.store(2, Ordering::SeqCst);
            pipe::write_all(writer, b"stale").await.unwrap();
        };

        // Act
        tokio::join!(
            publish(vec![("text/plain", reader)], false, &epoch, 1, &changes),
            write,
        );

        // Assert
        assert!(matches!(
            receiver.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }
}
