// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::{Seek, SeekFrom, Write};
use std::sync::atomic::Ordering;

use a7n_core::files::{EntryKind, ValidManifest};
use a7n_platform::transfer_fs::{OpenMode, open_file};
use a7n_proto::{
    limits,
    v1::{FileChunk, FileEnd, transfer_frame::Frame},
};
use sha2::Digest;

use super::super::{
    Failure, Progress, Result,
    staging::{PartialFile, Staging},
};

pub(in crate::daemon::files) fn apply(
    state: &mut Staging,
    manifest: &ValidManifest,
    frame: Frame,
    progress: &Progress,
) -> Result<bool> {
    match frame {
        Frame::Chunk(chunk) => {
            apply_chunk(state, manifest, &chunk, progress)?;
        }
        Frame::FileEnd(end) => {
            apply_file_end(state, manifest, &end, progress)?;
        }
        Frame::Done(_) if state.files.iter().flatten().all(|file| file.ended) => return Ok(true),
        Frame::Abort(_) => return Err(Failure::Cancelled),
        _ => return Err(Failure::Invalid),
    }
    Ok(false)
}

fn apply_chunk(
    state: &mut Staging,
    manifest: &ValidManifest,
    chunk: &FileChunk,
    progress: &Progress,
) -> Result<()> {
    let index = usize::try_from(chunk.file_index).map_err(|_| Failure::Invalid)?;
    let file = state
        .files
        .get_mut(index)
        .and_then(Option::as_mut)
        .ok_or(Failure::Invalid)?;
    let EntryKind::File { size, .. } = manifest.entries()[index].kind else {
        return Err(Failure::Invalid);
    };
    if file.ended
        || chunk.offset != file.written
        || chunk.data.len() > limits::CHUNK_MAX
        || (chunk.data.len() as u64) > size.saturating_sub(file.written)
    {
        return Err(Failure::Invalid);
    }
    open_partial(&mut state.open, file, chunk.file_index)?;
    state
        .open
        .as_mut()
        .ok_or(Failure::Io)?
        .1
        .write_all(&chunk.data)
        .map_err(|_| Failure::Io)?;
    file.hash.update(&chunk.data);
    file.written += chunk.data.len() as u64;
    progress
        .bytes
        .fetch_add(chunk.data.len() as u64, Ordering::Relaxed);
    Ok(())
}

fn open_partial(
    open: &mut Option<(u32, std::fs::File)>,
    file: &PartialFile,
    file_index: u32,
) -> Result<()> {
    if open.as_ref().is_none_or(|(index, _)| *index != file_index) {
        let mut handle = open_file(&file.path, OpenMode::Write).map_err(|_| Failure::Io)?;
        if handle.metadata().map_err(|_| Failure::Io)?.len() != file.written {
            return Err(Failure::Io);
        }
        handle
            .seek(SeekFrom::Start(file.written))
            .map_err(|_| Failure::Io)?;
        *open = Some((file_index, handle));
    }
    Ok(())
}

fn apply_file_end(
    state: &mut Staging,
    manifest: &ValidManifest,
    end: &FileEnd,
    progress: &Progress,
) -> Result<()> {
    let index = usize::try_from(end.file_index).map_err(|_| Failure::Invalid)?;
    let file = state
        .files
        .get_mut(index)
        .and_then(Option::as_mut)
        .ok_or(Failure::Invalid)?;
    let EntryKind::File { size, sha256 } = manifest.entries()[index].kind else {
        return Err(Failure::Invalid);
    };
    if file.ended {
        return Err(Failure::Invalid);
    }
    if file.written != size
        || end.sha256 != sha256
        || file.hash.clone().finalize().as_slice() != sha256
    {
        return Err(Failure::Integrity);
    }
    if let Some((index, handle)) = state.open.take() {
        handle.sync_all().map_err(|_| Failure::Io)?;
        if index != end.file_index {
            state.open = Some((index, handle));
        }
    }
    file.ended = true;
    progress.files.fetch_add(1, Ordering::Relaxed);
    Ok(())
}
