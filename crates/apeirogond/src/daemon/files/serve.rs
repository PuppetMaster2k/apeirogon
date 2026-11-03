// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, atomic::Ordering};

use a7n_core::files::ValidManifest;
use a7n_ipc::TransferDirection;
use a7n_proto::v1::{
    AbortReason, FileChunk, FileEnd, TransferAbort, TransferDone, TransferOpen, TransferRequest,
    transfer_frame::Frame, transfer_request::Source,
};
use a7n_proto::{MachineId, limits};
use quinn::{Connection, SendStream};
use tokio::sync::watch;
use tokio::time::timeout;

use super::{Active, Daemon, Event, Failure, Offer, Progress, Result};
use crate::daemon::clipboard::fetch::write;

impl Daemon {
    pub(in crate::daemon) fn files_request(
        &mut self,
        peer: MachineId,
        request: &TransferRequest,
    ) -> bool {
        let Some(Source::Clipboard(reference)) = &request.source else {
            return false;
        };
        if !reference.send {
            return false;
        }
        if reference.origin != self.identity.machine_id.as_bytes() {
            return false;
        }
        let Some(offer) = self
            .files
            .offers
            .get(&(peer, reference.generation))
            .cloned()
        else {
            return false;
        };
        let Some(transfers) = self.clipboard.links.get(&peer) else {
            return true;
        };
        let Ok(permit) = transfers.send_slots.clone().try_acquire_owned() else {
            crate::log::event!("kind=file_transfer_refused reason=busy");
            return true;
        };
        let link = transfers.id;
        let duplicate = self.files.active.values().any(|active| {
            active.peer == peer
                && active.link == link
                && active.direction == TransferDirection::Send
                && (active.wire_id == request.transfer_id || active.offer == reference.generation)
        });
        if self.check_file_send(peer).is_err() || request.item_index != 0 || duplicate {
            let connection = self.links.by_peer[&peer].connection.clone();
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
        let manifest =
            ValidManifest::try_from(offer.wire.files.as_ref().expect("validated offer manifest"))
                .expect("validated offer manifest");
        let progress = Arc::new(Progress::new(&manifest, offer.wire.items[0].size));
        let (cancel, mut cancelled) = watch::channel(Failure::Cancelled);
        self.files.active.insert(
            id,
            Active {
                peer,
                link,
                wire_id: request.transfer_id,
                offer: reference.generation,
                direction: TransferDirection::Send,
                cancel,
                progress: progress.clone(),
            },
        );
        let connection = self.links.by_peer[&peer].connection.clone();
        let request = request.clone();
        let events = self.events.clone();
        self.tasks.spawn(async move {
            let runtime = tokio::runtime::Handle::current();
            let result = tokio::task::spawn_blocking(move || {
                runtime.block_on(async {
                    tokio::select! {
                        biased;
                        _ = cancelled.changed() => Err(*cancelled.borrow()),
                        result = send(&connection, &request, &offer, &progress) => result,
                    }
                })
            })
            .await
            .unwrap_or(Err(Failure::Io));
            drop(permit);
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
    let mut stream = timeout(limits::FRAME_READ_TIMEOUT, connection.open_uni())
        .await
        .map_err(|_| Failure::LinkLost)?
        .map_err(|_| Failure::LinkLost)?;
    stream.set_priority(-1).map_err(|_| Failure::Io)?;
    let mut buf = Vec::new();
    frame(
        &mut stream,
        &mut buf,
        Frame::Open(TransferOpen { transfer_id: id }),
    )
    .await?;
    Ok((stream, buf))
}

async fn refuse(connection: &Connection, id: u64) -> Result<()> {
    let (mut stream, mut buf) = open(connection, id).await?;
    abort(&mut stream, &mut buf, AbortReason::Refused).await
}

async fn abort(stream: &mut SendStream, buf: &mut Vec<u8>, reason: AbortReason) -> Result<()> {
    frame(
        stream,
        buf,
        Frame::Abort(TransferAbort {
            reason: reason.into(),
            detail: String::new(),
        }),
    )
    .await?;
    stream.finish().map_err(|_| Failure::LinkLost)
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
