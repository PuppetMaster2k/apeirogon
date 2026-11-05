// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, atomic::Ordering};

use a7n_ipc::TransferDirection;
use a7n_proto::v1::{
    AbortReason, FileChunk, FileEnd, TransferDone, TransferRequest, transfer_frame::Frame,
    transfer_request::Source,
};
use a7n_proto::{MachineId, limits};
use quinn::{Connection, SendStream};
use tokio::sync::watch;
use tokio::time::timeout;

use super::{Active, Daemon, Event, Failure, Kind, Offer, Progress, Result};
use crate::daemon::transfer::{self, TransferError, write};

impl Daemon {
    pub(in crate::daemon) fn files_request(
        &mut self,
        peer: MachineId,
        request: &TransferRequest,
    ) -> bool {
        let Some(Source::Clipboard(reference)) = &request.source else {
            return false;
        };
        if reference.origin != self.identity.machine_id.as_bytes() {
            return false;
        }
        let (offer, kind) = if reference.send {
            let offer = self.files.offers.get(&(peer, reference.generation));
            (offer.cloned(), Kind::Offer)
        } else {
            // A superseded generation is not found here and falls through to
            // the clipboard path, which answers ABORT_REASON_NOT_FOUND.
            let offer = self.clipboard_files_offer(peer, reference.generation, request.item_index);
            (offer, Kind::ClipboardSend)
        };
        let Some(offer) = offer else {
            return false;
        };
        let Some(handle) = self.links.by_peer.get(&peer) else {
            return true;
        };
        let connection = handle.connection.clone();
        let Some(transfers) = self.clipboard.links.get(&peer) else {
            return true;
        };
        let Ok(permit) = transfers.slots.clone().try_acquire_owned() else {
            crate::log::event!("kind=file_transfer_refused reason=busy");
            return true;
        };
        let link = transfers.id;
        let duplicate = self.files.active.values().any(|active| {
            active.peer == peer
                && active.link == link
                && active.direction == TransferDirection::Send
                && (active.wire_id == request.transfer_id
                    || (active.offer == reference.generation && active.kind == kind))
        });
        let unsupported =
            kind == Kind::Offer && (self.check_file_send(peer).is_err() || request.item_index != 0);
        if unsupported || duplicate {
            let wire_id = request.transfer_id;
            self.tasks.spawn(async move {
                let _ = refuse(&connection, wire_id).await;
                drop(permit);
            });
            return true;
        }
        let Some(id) = self.files.next_transfer.checked_add(1) else {
            return true;
        };
        self.files.next_transfer = id;
        let progress = Arc::new(Progress::new(&offer.manifest, offer.wire.items[0].size));
        let (cancel, mut cancelled) = watch::channel(Failure::Cancelled);
        self.files.active.insert(
            id,
            Active {
                peer,
                link,
                wire_id: request.transfer_id,
                offer: reference.generation,
                kind,
                direction: TransferDirection::Send,
                cancel,
                progress: progress.clone(),
            },
        );
        let request = request.clone();
        let events = self.events.clone();
        self.tasks.spawn(async move {
            let result = super::worker::run(async move {
                tokio::select! {
                    biased;
                    _ = cancelled.changed() => Err(*cancelled.borrow()),
                    result = send(&connection, &request, &offer, &progress) => result,
                }
            })
            .await;
            drop(permit);
            let result = result.map(|()| Vec::new());
            let _ = events.send(Event::FilesFinished { id, result }).await;
        });
        true
    }
}

async fn frame(stream: &mut SendStream, buf: &mut Vec<u8>, value: Frame) -> Result<()> {
    timeout(limits::FRAME_READ_TIMEOUT, write(stream, buf, value))
        .await
        .map_err(|_| Failure::LinkLost)?
        .map_err(|_| Failure::LinkLost)
}

async fn open(connection: &Connection, id: u64) -> Result<(SendStream, Vec<u8>)> {
    transfer::open(connection, id, Some(limits::FRAME_READ_TIMEOUT))
        .await
        .map_err(|error| match error {
            TransferError::Priority => Failure::Io,
            _ => Failure::LinkLost,
        })
}

async fn refuse(connection: &Connection, id: u64) -> Result<()> {
    let (mut stream, mut buf) = open(connection, id).await?;
    abort(&mut stream, &mut buf, AbortReason::Refused).await
}

async fn abort(stream: &mut SendStream, buf: &mut Vec<u8>, reason: AbortReason) -> Result<()> {
    transfer::abort(stream, buf, reason, Some(limits::FRAME_READ_TIMEOUT))
        .await
        .map_err(|_| Failure::LinkLost)
}

async fn send(
    connection: &Connection,
    request: &TransferRequest,
    offer: &Offer,
    progress: &Progress,
) -> Result<()> {
    let (mut stream, mut buf) = open(connection, request.transfer_id).await?;
    let result = send_files(&mut stream, &mut buf, request, offer, progress).await;
    if let Err(reason) = result {
        let abort_reason = if reason == Failure::Invalid {
            AbortReason::Refused
        } else {
            AbortReason::IoError
        };
        let _ = abort(&mut stream, &mut buf, abort_reason).await;
        return result;
    }
    frame(&mut stream, &mut buf, Frame::Done(TransferDone {})).await?;
    stream.finish().map_err(|_| Failure::LinkLost)?;
    timeout(limits::FRAME_READ_TIMEOUT, stream.stopped())
        .await
        .map_err(|_| Failure::LinkLost)?
        .map_err(|_| Failure::LinkLost)?;
    Ok(())
}

async fn send_files(
    stream: &mut SendStream,
    buf: &mut Vec<u8>,
    request: &TransferRequest,
    offer: &Offer,
    progress: &Progress,
) -> Result<()> {
    let manifest = offer.wire.files.as_ref().ok_or(Failure::Invalid)?;
    let mut resume = HashMap::new();
    for part in &request.resume {
        let entry = manifest
            .entries
            .get(part.file_index as usize)
            .ok_or(Failure::Invalid)?;
        if entry.kind() != a7n_proto::v1::FileKind::File
            || part.offset > entry.size
            || resume.insert(part.file_index, part.offset).is_some()
        {
            return Err(Failure::Invalid);
        }
    }
    let mut data = vec![0; limits::CHUNK_MAX];
    for (index, entry) in manifest.entries.iter().enumerate() {
        if entry.kind() == a7n_proto::v1::FileKind::Directory {
            continue;
        }
        let index = u32::try_from(index).map_err(|_| Failure::Invalid)?;
        let path = &offer.paths[index as usize];
        let metadata = std::fs::symlink_metadata(path).map_err(|_| Failure::Io)?;
        if !metadata.is_file() || metadata.len() != entry.size {
            return Err(Failure::Io);
        }
        let mut file =
            a7n_platform::transfer_fs::open_file(path, false, false).map_err(|_| Failure::Io)?;
        if file.metadata().map_err(|_| Failure::Io)?.len() != entry.size {
            return Err(Failure::Io);
        }
        let mut offset = resume.get(&index).copied().unwrap_or(0);
        file.seek(SeekFrom::Start(offset))
            .map_err(|_| Failure::Io)?;
        progress.bytes.fetch_add(offset, Ordering::Relaxed);
        while offset < entry.size {
            let remaining = usize::try_from((entry.size - offset).min(limits::CHUNK_MAX as u64))
                .map_err(|_| Failure::Invalid)?;
            file.read_exact(&mut data[..remaining])
                .map_err(|_| Failure::Io)?;
            frame(
                stream,
                buf,
                Frame::Chunk(FileChunk {
                    file_index: index,
                    offset,
                    data: data[..remaining].to_vec(),
                }),
            )
            .await?;
            offset += remaining as u64;
            progress
                .bytes
                .fetch_add(remaining as u64, Ordering::Relaxed);
        }
        frame(
            stream,
            buf,
            Frame::FileEnd(FileEnd {
                file_index: index,
                sha256: entry.sha256.clone(),
            }),
        )
        .await?;
        progress.files.fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}
