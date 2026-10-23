// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_platform::clipboard::PasteRequest;
use a7n_proto::v1::{
    AbortReason, ClipboardItem, ClipboardRef, FileChunk, FileEnd, TransferAbort, TransferDone,
    TransferFrame, TransferOpen, TransferRequest, envelope::Body, transfer_frame::Frame,
    transfer_request::Source,
};
use a7n_proto::{FrameKind, MachineId, decode_frame, encode_frame, limits};
use quinn::{Connection, RecvStream, SendStream};
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;
use tokio::time::timeout;

use crate::daemon::{Daemon, Event, TransferStream};

impl Daemon {
    pub(in crate::daemon) fn clipboard_paste(&mut self, request: PasteRequest) {
        let peer = request.generation.origin;
        let item = self
            .clipboard
            .announce
            .as_ref()
            .and_then(|wire| wire.items.get(request.item_index as usize).cloned());
        if self.clipboard.current != Some(request.generation)
            || peer == self.identity.machine_id
            || !matches!(
                self.clipboard_policy(peer),
                super::ClipboardPolicy::Both | super::ClipboardPolicy::Receive
            )
            || !self.links.by_peer.contains_key(&peer)
            || item.is_none()
        {
            reject(request, "unavailable");
            return;
        }
        let Some(transfers) = self.clipboard.links.get_mut(&peer) else {
            reject(request, "link_lost");
            return;
        };
        let Ok(permit) = transfers.fetch_slots.clone().try_acquire_owned() else {
            reject(request, "busy");
            return;
        };
        transfers.pending.retain(|_, reply| !reply.is_closed());
        let Some(next) = transfers.next.checked_add(1) else {
            reject(request, "transfer_id_exhausted");
            return;
        };
        transfers.next = next;
        let id = transfers.id;
        let (sender, receiver) = oneshot::channel();
        transfers.pending.insert(next, sender);
        let connection = self.links.by_peer[&peer].connection.clone();
        self.send_input(
            peer,
            Body::TransferRequest(TransferRequest {
                transfer_id: next,
                source: Some(Source::Clipboard(ClipboardRef {
                    origin: peer.as_bytes().to_vec(),
                    generation: request.generation.generation,
                })),
                item_index: request.item_index,
                resume: Vec::new(),
            }),
        );
        let events = self.events.clone();
        let item = item.expect("item checked above");
        self.tasks.spawn(async move {
            let _permit = permit;
            let result = timeout(limits::CLIPBOARD_FETCH_TIMEOUT, async {
                let (mut stream, mut buf) = receiver.await.map_err(|_| "link_lost")?;
                receive(&mut stream, &mut buf, &item).await
            })
            .await
            .unwrap_or(Err("timeout"));
            if let Err(reason) = result {
                crate::log::event!("kind=clipboard_fetch_failed reason={reason}");
                if reason == "frame" {
                    connection.close(0u32.into(), b"");
                }
            }
            let _ = events
                .send(Event::ClipboardFetched {
                    peer,
                    link: id,
                    request,
                    data: result.ok(),
                })
                .await;
        });
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
            && matches!(
                self.clipboard_policy(peer),
                super::ClipboardPolicy::Both | super::ClipboardPolicy::Receive
            );
        if valid {
            let _ = request.reply.send(data);
        } else {
            reject(request, "stale");
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
        let events = self.events.clone();
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
                Ok(Err("frame")) => connection.close(0u32.into(), b""),
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
        let permit = transfers.send_slots.clone().try_acquire_owned().ok();
        let result = if !self.clipboard_can_send(peer) || permit.is_none() {
            Err(AbortReason::Refused)
        } else {
            self.clipboard.announce.as_ref().filter(|wire| {
                wire.origin == self.identity.machine_id.as_bytes()
                    && matches!(&request.source, Some(Source::Clipboard(reference))
                        if reference.origin == wire.origin && reference.generation == wire.generation)
                    && request.resume.is_empty()
            }).and_then(|_| self.clipboard.own_items.get(request.item_index as usize))
                .map(|item| item.data.clone()).ok_or(AbortReason::NotFound)
        };
        let connection = link.connection.clone();
        let transfer_id = request.transfer_id;
        self.tasks.spawn(async move {
            let _ = timeout(
                limits::CLIPBOARD_FETCH_TIMEOUT,
                send(&connection, transfer_id, result),
            )
            .await;
            drop(permit);
        });
    }
}

fn reject(request: PasteRequest, reason: &str) {
    crate::log::event!("kind=clipboard_fetch_failed reason={reason}");
    let _ = request.reply.send(None);
}

async fn write(
    stream: &mut SendStream,
    buf: &mut Vec<u8>,
    frame: Frame,
) -> Result<(), &'static str> {
    buf.clear();
    encode_frame(&TransferFrame { frame: Some(frame) }, buf);
    stream.write_all(buf).await.map_err(|_| "write")
}

async fn send(
    connection: &Connection,
    transfer_id: u64,
    data: Result<Vec<u8>, AbortReason>,
) -> Result<(), &'static str> {
    let mut stream = connection.open_uni().await.map_err(|_| "open")?;
    stream.set_priority(-1).map_err(|_| "priority")?;
    let mut buf = Vec::new();
    write(
        &mut stream,
        &mut buf,
        Frame::Open(TransferOpen { transfer_id }),
    )
    .await?;
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
            write(
                &mut stream,
                &mut buf,
                Frame::Abort(TransferAbort {
                    reason: reason.into(),
                    detail: String::new(),
                }),
            )
            .await?;
        }
    }
    stream.finish().map_err(|_| "finish")?;
    stream.stopped().await.map_err(|_| "stopped")?;
    Ok(())
}

async fn read(stream: &mut RecvStream, buf: &mut Vec<u8>) -> Result<Option<Frame>, &'static str> {
    loop {
        if let Some((frame, used)) =
            decode_frame::<TransferFrame>(buf, FrameKind::Transfer).map_err(|_| "frame")?
        {
            buf.drain(..used);
            return frame.frame.map(Some).ok_or("frame");
        }
        // A bounded read prevents a peer from making us buffer arbitrarily beyond a frame.
        let chunk = stream
            .read_chunk(
                limits::TRANSFER_FRAME_MAX.saturating_sub(buf.len()).max(1),
                true,
            )
            .await
            .map_err(|_| "read")?;
        match chunk {
            Some(chunk) => buf.extend_from_slice(&chunk.bytes),
            None if buf.is_empty() => return Ok(None),
            None => return Err("truncated"),
        }
    }
}

async fn receive(
    stream: &mut RecvStream,
    buf: &mut Vec<u8>,
    item: &ClipboardItem,
) -> Result<Vec<u8>, &'static str> {
    let mut verifier = Verifier::new(item)?;
    loop {
        let frame = read(stream, buf).await?.ok_or("truncated")?;
        if verifier.apply(frame)? {
            if read(stream, buf).await?.is_some() {
                return Err("trailing_frame");
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
    fn new(item: &'a ClipboardItem) -> Result<Self, &'static str> {
        if item.size > limits::CLIPBOARD_TEXT_MAX as u64 {
            return Err("size");
        }
        Ok(Self {
            item,
            data: Vec::new(),
            ended: false,
        })
    }

    fn apply(&mut self, frame: Frame) -> Result<bool, &'static str> {
        match frame {
            Frame::Chunk(chunk) if !self.ended => {
                if chunk.file_index != 0
                    || chunk.offset != self.data.len() as u64
                    || chunk.data.len() > limits::CHUNK_MAX
                    || (self.data.len() + chunk.data.len()) as u64 > self.item.size
                {
                    return Err("chunk");
                }
                self.data.extend_from_slice(&chunk.data);
            }
            Frame::FileEnd(end) if !self.ended => {
                if end.file_index != 0
                    || self.data.len() as u64 != self.item.size
                    || end.sha256 != self.item.sha256
                    || Sha256::digest(&self.data).as_slice() != self.item.sha256
                {
                    return Err("integrity");
                }
                self.ended = true;
            }
            Frame::Done(_) if self.ended => return Ok(true),
            Frame::Abort(abort) => {
                return Err(match abort.reason() {
                    AbortReason::NotFound => "not_found",
                    AbortReason::Refused => "refused",
                    _ => "abort",
                });
            }
            _ => return Err("ordering"),
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests;
