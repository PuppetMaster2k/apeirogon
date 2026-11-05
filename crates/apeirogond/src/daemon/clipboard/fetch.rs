// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_core::clipboard::{FILES, IMAGE_PNG, limit_for, validate_png};
use a7n_platform::clipboard::PasteRequest;
use a7n_proto::v1::{
    AbortReason, ClipboardItem, ClipboardRef, FileChunk, FileEnd, TransferDone, TransferRequest,
    envelope::Body, transfer_frame::Frame, transfer_request::Source,
};
use a7n_proto::{MachineId, limits};
use quinn::Connection;
use sha2::{Digest, Sha256};
use tokio::sync::{OwnedSemaphorePermit, oneshot};
use tokio::time::timeout;

use crate::daemon::transfer::{self, TransferError, read, write};
use crate::daemon::{Daemon, Event, TransferStream};

struct Fetch {
    connection: Connection,
    permit: OwnedSemaphorePermit,
    link: usize,
    transfer_id: u64,
    receiver: oneshot::Receiver<(quinn::RecvStream, Vec<u8>)>,
}

impl Daemon {
    pub(in crate::daemon) fn clipboard_paste(&mut self, request: PasteRequest) {
        let peer = request.generation.origin;
        let item = self
            .clipboard
            .announce
            .as_ref()
            .and_then(|wire| wire.items.get(request.item_index as usize).cloned());
        let Some(item) = item.filter(|_| {
            self.clipboard.current == Some(request.generation)
                && peer != self.identity.machine_id
                && self.clipboard_policy(peer).receives()
                && self.links.by_peer.contains_key(&peer)
        }) else {
            reject(request, TransferError::Unavailable);
            return;
        };
        if item.mime == FILES {
            self.paste_files(request);
            return;
        }
        let fetch = match self.prepare_clipboard_fetch(peer) {
            Ok(fetch) => fetch,
            Err(error) => {
                reject(request, error);
                return;
            }
        };
        self.start_clipboard_fetch(request, item, fetch);
    }

    fn prepare_clipboard_fetch(&mut self, peer: MachineId) -> Result<Fetch, TransferError> {
        let Some(link) = self.links.by_peer.get(&peer) else {
            return Err(TransferError::LinkLost);
        };
        let connection = link.connection.clone();
        let Some(transfers) = self.clipboard.links.get_mut(&peer) else {
            return Err(TransferError::LinkLost);
        };
        let Ok(permit) = transfers.slots.clone().try_acquire_owned() else {
            return Err(TransferError::Busy);
        };
        transfers.pending.retain(|_, reply| !reply.is_closed());
        let Some(next) = transfers.next.checked_add(2) else {
            return Err(TransferError::TransferIdExhausted);
        };
        transfers.next = next;
        let id = transfers.id;
        let (sender, receiver) = oneshot::channel();
        transfers.pending.insert(next, sender);
        Ok(Fetch {
            connection,
            permit,
            link: id,
            transfer_id: next,
            receiver,
        })
    }

    fn start_clipboard_fetch(&mut self, request: PasteRequest, item: ClipboardItem, job: Fetch) {
        let peer = request.generation.origin;
        let Fetch {
            connection,
            permit,
            link,
            transfer_id,
            receiver,
        } = job;
        self.request_clipboard_fetch(&request, transfer_id);
        let events = self.mailboxes.events.clone();
        let task = self.tasks.spawn(async move {
            let result = fetch(receiver, &item, &connection).await;
            drop(permit);
            let _ = events
                .send(Event::ClipboardFetched {
                    peer,
                    link,
                    request,
                    data: result.ok(),
                })
                .await;
        });
        if let Some(transfers) = self.clipboard.links.get_mut(&peer) {
            transfers.fetch_tasks.retain(|_, task| !task.is_finished());
            transfers.fetch_tasks.insert(transfer_id, task);
        }
    }

    fn request_clipboard_fetch(&mut self, request: &PasteRequest, transfer_id: u64) {
        let peer = request.generation.origin;
        self.send_input(
            peer,
            Body::TransferRequest(TransferRequest {
                transfer_id,
                source: Some(Source::Clipboard(ClipboardRef {
                    origin: peer.as_bytes().to_vec(),
                    generation: request.generation.generation,
                    send: false,
                })),
                item_index: request.item_index,
                resume: Vec::new(),
            }),
        );
    }

    pub(in crate::daemon) fn clipboard_fetched(
        &mut self,
        peer: MachineId,
        link: usize,
        request: PasteRequest,
        data: Option<Vec<u8>>,
    ) {
        let valid = self.is_current(peer, link)
            && self.clipboard.current == Some(request.generation)
            && self.clipboard.handle.is_some()
            && self.clipboard.announce.is_some()
            && self.clipboard_policy(peer).receives();
        self.start_waiting_files(peer);
        if valid {
            let _ = request.reply.send(data);
        } else {
            reject(request, TransferError::Stale);
        }
    }

    pub(in crate::daemon) fn clipboard_stream(&mut self, (peer, id, mut stream): TransferStream) {
        let Some(transfers) = self.clipboard.links.get(&peer).filter(|link| link.id == id) else {
            return;
        };
        let Ok(permit) = transfers.read_slots.clone().try_acquire_owned() else {
            return;
        };
        let Some(link) = self.links.by_peer.get(&peer) else {
            return;
        };
        let connection = link.connection.clone();
        let events = self.mailboxes.events.clone();
        self.tasks.spawn(async move {
            let mut buf = Vec::new();
            match timeout(limits::CLIPBOARD_FETCH_TIMEOUT, read(&mut stream, &mut buf)).await {
                Ok(Ok(Some(Frame::Open(open)))) => {
                    let _ = events
                        .send(Event::ClipboardOpened(
                            (peer, id, stream),
                            open.transfer_id,
                            buf,
                        ))
                        .await;
                }
                Ok(Err(TransferError::Frame)) => connection.close(crate::link::CLOSE_NORMAL, b""),
                _ => crate::log::event!("kind=clipboard_fetch_failed reason=open"),
            }
            drop(permit);
        });
    }

    pub(in crate::daemon) fn clipboard_opened(
        &mut self,
        (peer, id, stream): TransferStream,
        transfer_id: u64,
        buf: Vec<u8>,
    ) {
        if let Some(sender) = self
            .clipboard
            .links
            .get_mut(&peer)
            .filter(|link| link.id == id)
            .and_then(|link| link.pending.remove(&transfer_id))
        {
            let _ = sender.send((stream, buf));
        } else {
            crate::log::event!("kind=clipboard_fetch_failed reason=unknown_transfer");
        }
    }

    pub(in crate::daemon) fn clipboard_request(
        &mut self,
        peer: MachineId,
        request: &TransferRequest,
    ) {
        let Some(link) = self.links.by_peer.get(&peer) else {
            return;
        };
        let Some(transfers) = self.clipboard.links.get(&peer) else {
            return;
        };
        let Ok(permit) = transfers.slots.clone().try_acquire_owned() else {
            crate::log::event!("kind=clipboard_fetch_failed reason=busy");
            return;
        };
        let result = if self.clipboard_can_send(peer) {
            self.clipboard.announce.as_ref().filter(|wire| {
                wire.origin == self.identity.machine_id.as_bytes()
                    && matches!(&request.source, Some(Source::Clipboard(reference))
                        if !reference.send && reference.origin == wire.origin && reference.generation == wire.generation)
                    && request.resume.is_empty()
            }).and_then(|_| self.clipboard.own_items.iter()
                .filter(|item| item.mime != IMAGE_PNG || self.clipboard_can_send_image(peer))
                .nth(request.item_index as usize))
                .map(|item| item.data.clone()).ok_or(AbortReason::NotFound)
        } else {
            Err(AbortReason::Refused)
        };
        let connection = link.connection.clone();
        let transfer_id = request.transfer_id;
        let task = self.tasks.spawn(async move {
            let _ = timeout(
                limits::CLIPBOARD_FETCH_TIMEOUT,
                send(&connection, transfer_id, result),
            )
            .await;
            drop(permit);
        });
        if let Some(transfers) = self.clipboard.links.get_mut(&peer) {
            transfers.send_tasks.retain(|_, task| !task.is_finished());
            if let Some(old) = transfers.send_tasks.insert(transfer_id, task) {
                old.abort();
            }
        }
    }
}

async fn fetch(
    receiver: oneshot::Receiver<(quinn::RecvStream, Vec<u8>)>,
    item: &ClipboardItem,
    connection: &Connection,
) -> Result<Vec<u8>, TransferError> {
    let result = timeout(limits::CLIPBOARD_FETCH_TIMEOUT, async {
        let (mut stream, mut buf) = receiver.await.map_err(|_| TransferError::LinkLost)?;
        receive(&mut stream, &mut buf, item).await
    })
    .await
    .unwrap_or(Err(TransferError::Timeout));
    if let Err(reason) = result {
        crate::log::event!("kind=clipboard_fetch_failed reason={reason}");
        if reason == TransferError::Frame {
            connection.close(crate::link::CLOSE_NORMAL, b"");
        }
    }
    result
}

pub(in crate::daemon) fn reject(request: PasteRequest, reason: TransferError) {
    crate::log::event!("kind=clipboard_fetch_failed reason={reason}");
    let _ = request.reply.send(None);
}

async fn send(
    connection: &Connection,
    transfer_id: u64,
    data: Result<Vec<u8>, AbortReason>,
) -> Result<(), TransferError> {
    let (mut stream, mut buf) = transfer::open(connection, transfer_id, None).await?;
    match data {
        Ok(data) => {
            for (index, chunk) in data.chunks(limits::CHUNK_MAX).enumerate() {
                write(
                    &mut stream,
                    &mut buf,
                    Frame::Chunk(FileChunk {
                        file_index: 0,
                        offset: (index * limits::CHUNK_MAX) as u64,
                        data: chunk.to_vec(),
                    }),
                )
                .await?;
            }
            write(
                &mut stream,
                &mut buf,
                Frame::FileEnd(FileEnd {
                    file_index: 0,
                    sha256: Sha256::digest(&data).to_vec(),
                }),
            )
            .await?;
            write(&mut stream, &mut buf, Frame::Done(TransferDone {})).await?;
        }
        Err(reason) => {
            transfer::abort(&mut stream, &mut buf, reason, None).await?;
            stream.stopped().await.map_err(|_| TransferError::Stopped)?;
            return Ok(());
        }
    }
    stream.finish().map_err(|_| TransferError::Finish)?;
    stream.stopped().await.map_err(|_| TransferError::Stopped)?;
    Ok(())
}

async fn receive(
    stream: &mut impl crate::frame::FrameStream,
    buf: &mut Vec<u8>,
    item: &ClipboardItem,
) -> Result<Vec<u8>, TransferError> {
    let mut verifier = Verifier::new(item)?;
    loop {
        let frame = read(stream, buf).await?.ok_or(TransferError::Truncated)?;
        if verifier.apply(frame)? {
            if read(stream, buf).await?.is_some() {
                return Err(TransferError::TrailingFrame);
            }
            return Ok(verifier.data);
        }
    }
}

struct Verifier<'a> {
    item: &'a ClipboardItem,
    data: Vec<u8>,
    ended: bool,
}

impl<'a> Verifier<'a> {
    fn new(item: &'a ClipboardItem) -> Result<Self, TransferError> {
        if item.size > limit_for(&item.mime) as u64 {
            return Err(TransferError::Size);
        }
        Ok(Self {
            item,
            data: Vec::new(),
            ended: false,
        })
    }

    fn apply(&mut self, frame: Frame) -> Result<bool, TransferError> {
        match frame {
            Frame::Chunk(chunk) if !self.ended => {
                if chunk.file_index != 0
                    || chunk.offset != self.data.len() as u64
                    || chunk.data.len() > limits::CHUNK_MAX
                    || (self.data.len() + chunk.data.len()) as u64 > self.item.size
                {
                    return Err(TransferError::Chunk);
                }
                self.data.extend_from_slice(&chunk.data);
            }
            Frame::FileEnd(end) if !self.ended => {
                if end.file_index != 0
                    || self.data.len() as u64 != self.item.size
                    || end.sha256 != self.item.sha256
                    || Sha256::digest(&self.data).as_slice() != self.item.sha256
                {
                    return Err(TransferError::Integrity);
                }
                if self.item.mime == IMAGE_PNG && validate_png(&self.data).is_err() {
                    return Err(TransferError::Image);
                }
                self.ended = true;
            }
            Frame::Done(_) if self.ended => return Ok(true),
            Frame::Abort(abort) => {
                return Err(match abort.reason() {
                    AbortReason::NotFound => TransferError::NotFound,
                    AbortReason::Refused => TransferError::Refused,
                    _ => TransferError::Abort,
                });
            }
            _ => return Err(TransferError::Ordering),
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests;
