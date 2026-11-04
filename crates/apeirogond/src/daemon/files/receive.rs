// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;
use std::sync::{Arc, atomic::Ordering};

use a7n_core::files::ValidManifest;
use a7n_proto::limits;
use quinn::{Connection, RecvStream};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::timeout;

use super::{Event, Failure, Progress, Result, staging};
use crate::daemon::clipboard::fetch::read;

mod verify;
pub(super) use verify::apply;

const FREE_SPACE_MARGIN: u64 = 256 * 1024 * 1024;

pub(super) fn check_space(
    directory: &std::path::Path,
    total: u64,
    resumed: u64,
    available_space: impl FnOnce(&std::path::Path) -> std::io::Result<u64>,
) -> Result<()> {
    let Ok(available) = available_space(directory) else {
        crate::log::event!("kind=file_receive_space_query_failed");
        return Ok(());
    };
    let needed = total.saturating_sub(resumed).checked_add(FREE_SPACE_MARGIN);
    if needed.is_none_or(|needed| available < needed) {
        return Err(Failure::NoSpace);
    }
    Ok(())
}

pub(super) struct Receive {
    pub id: u64,
    pub manifest: ValidManifest,
    pub stage: PathBuf,
    pub destination: PathBuf,
    pub receiver: oneshot::Receiver<(RecvStream, Vec<u8>)>,
    pub cancelled: watch::Receiver<Failure>,
    pub progress: Arc<Progress>,
    pub events: mpsc::Sender<Event>,
    pub connection: Connection,
}

pub(super) async fn run(mut job: Receive) -> Result<Vec<PathBuf>> {
    let result = tokio::select! {
        biased;
        _ = job.cancelled.changed() => Err(*job.cancelled.borrow()),
        result = download(&job.manifest, &job.stage, &job.destination, &mut job.receiver, &job.progress, &job.events, job.id) => result,
    };
    if result.is_err() && !matches!(result, Err(Failure::LinkLost | Failure::NoSpace)) {
        let _ = std::fs::remove_dir_all(&job.stage);
    }
    if result == Err(Failure::Invalid) {
        // Malformed transfer framing closes the link, as for clipboard fetches.
        job.connection.close(0u32.into(), b"");
    }
    result
}

async fn download(
    manifest: &ValidManifest,
    stage: &std::path::Path,
    receive_dir: &std::path::Path,
    receiver: &mut oneshot::Receiver<(RecvStream, Vec<u8>)>,
    progress: &Progress,
    events: &mpsc::Sender<Event>,
    id: u64,
) -> Result<Vec<PathBuf>> {
    // Refuse fresh downloads before even creating staging on a full filesystem.
    if !stage.try_exists().map_err(|_| Failure::Io)? {
        check_space(
            receive_dir,
            progress.bytes_total,
            0,
            a7n_platform::transfer_fs::available_space,
        )?;
    }
    let mut partial = staging::prepare(stage, manifest)?;
    let resumed = partial
        .files
        .iter()
        .flatten()
        .map(|file| file.written)
        .sum();
    check_space(
        receive_dir,
        progress.bytes_total,
        resumed,
        a7n_platform::transfer_fs::available_space,
    )?;
    progress.bytes.store(resumed, Ordering::Relaxed);
    events
        .send(Event::FilesReady {
            id,
            resume: partial.resume.clone(),
        })
        .await
        .map_err(|_| Failure::LinkLost)?;
    let (mut stream, mut buf) = timeout(limits::FRAME_READ_TIMEOUT, receiver)
        .await
        .map_err(|_| Failure::LinkLost)?
        .map_err(|_| Failure::LinkLost)?;
    loop {
        let frame = timeout(limits::FRAME_READ_TIMEOUT, read(&mut stream, &mut buf))
            .await
            .map_err(|_| Failure::LinkLost)?
            .map_err(|reason| {
                if reason == "frame" {
                    Failure::Invalid
                } else {
                    Failure::LinkLost
                }
            })?
            .ok_or(Failure::LinkLost)?;
        if apply(&mut partial, manifest, frame, progress)? {
            break;
        }
    }
    if timeout(limits::FRAME_READ_TIMEOUT, read(&mut stream, &mut buf))
        .await
        .map_err(|_| Failure::LinkLost)?
        .map_err(|_| Failure::LinkLost)?
        .is_some()
    {
        return Err(Failure::Invalid);
    }
    // Close all file handles before publishing, including on Windows.
    drop(partial);
    staging::publish(stage, receive_dir, manifest)
}
