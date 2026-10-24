// SPDX-License-Identifier: GPL-3.0-or-later

use std::thread;
use std::time::{Duration, Instant};

use a7n_core::clipboard::Generation;
use a7n_proto::limits::CLIPBOARD_FETCH_TIMEOUT;
use tokio::sync::{mpsc, oneshot};

use super::PasteRequest;

const REPLY_INTERVAL: Duration = Duration::from_millis(10);

pub(super) fn fetch(
    generation: Generation,
    item_index: u32,
    pastes: &mpsc::Sender<PasteRequest>,
) -> Option<Vec<u8>> {
    let (reply, mut receiver) = oneshot::channel();
    pastes
        .try_send(PasteRequest {
            generation,
            item_index,
            reply,
        })
        .ok()?;
    let deadline = Instant::now() + CLIPBOARD_FETCH_TIMEOUT;
    loop {
        match receiver.try_recv() {
            Ok(bytes) => return bytes,
            Err(oneshot::error::TryRecvError::Empty) if Instant::now() < deadline => {
                thread::sleep(
                    REPLY_INTERVAL.min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            _ => return None,
        }
    }
}
