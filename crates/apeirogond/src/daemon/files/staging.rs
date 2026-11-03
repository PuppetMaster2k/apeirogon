// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use a7n_core::files::{EntryKind, Partial, ValidManifest, resume_plan, unique_name};
use a7n_platform::transfer_fs::{create_directory, open_file, rename_no_replace};
use a7n_proto::MachineId;
use a7n_proto::v1::FileResume;
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};

use super::{Failure, PARTIAL_DIR, PARTIAL_MAX_AGE, Result};

pub(super) fn roots(manifest: &ValidManifest) -> BTreeSet<&str> {
    manifest
        .entries()
        .iter()
        .filter_map(|entry| entry.path.segments().next())
        .collect()
}

pub(super) struct PartialFile {
    pub path: PathBuf,
    pub hash: Sha256,
    pub written: u64,
    pub ended: bool,
}

pub(super) struct Staging {
    pub files: Vec<Option<PartialFile>>,
    pub resume: Vec<FileResume>,
    pub open: Option<(u32, File)>,
}

pub(super) fn prepare(stage: &Path, manifest: &ValidManifest) -> Result<Staging> {
    create_directory(stage).map_err(|_| Failure::Io)?;
    let mut files = Vec::new();
    let mut partial = Vec::new();
    for (index, entry) in manifest.entries().iter().enumerate() {
        let path = stage.join(entry.path.as_str());
        if entry.kind == EntryKind::Directory {
            create_directory(&path).map_err(|_| Failure::Io)?;
            files.push(None);
            continue;
        }
        create_directory(path.parent().ok_or(Failure::Invalid)?).map_err(|_| Failure::Io)?;
        let mut file = match open_file(&path, true, true) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                open_file(&path, false, true).map_err(|_| Failure::Io)?
            }
            Err(_) => return Err(Failure::Io),
        };
        let EntryKind::File { size, .. } = entry.kind else {
            return Err(Failure::Invalid);
        };
        let mut written = file.metadata().map_err(|_| Failure::Io)?.len();
        if written > size {
            file.set_len(0).map_err(|_| Failure::Io)?;
            written = 0;
        }
        let mut hash = Sha256::new();
        hash_file(&mut file, &mut hash)?;
        if let EntryKind::File { sha256, .. } = entry.kind {
            if written == size && hash.clone().finalize().as_slice() != sha256 {
                file.set_len(0).map_err(|_| Failure::Io)?;
                hash = Sha256::new();
                written = 0;
            }
        }
        file.seek(SeekFrom::Start(written))
            .map_err(|_| Failure::Io)?;
        partial.push(Partial {
            file_index: u32::try_from(index).map_err(|_| Failure::Invalid)?,
            len: written,
        });
        files.push(Some(PartialFile {
            path,
            hash,
            written,
            ended: false,
        }));
    }
    let mut resume = resume_plan(manifest, &partial);
    // Only complete files whose full hash was verified above may skip all data.
    for part in &partial {
        if let EntryKind::File { size, .. } = manifest.entries()[part.file_index as usize].kind {
            if size > 0 && part.len == size {
                resume.push(FileResume {
                    file_index: part.file_index,
                    offset: size,
                });
            }
        }
    }
    Ok(Staging {
        files,
        resume,
        open: None,
    })
}

pub(super) fn hash_file(file: &mut File, hash: &mut Sha256) -> Result<()> {
    let mut bytes = vec![0; a7n_proto::limits::CHUNK_MAX];
    loop {
        let len = file.read(&mut bytes).map_err(|_| Failure::Io)?;
        if len == 0 {
            return Ok(());
        }
        hash.update(&bytes[..len]);
    }
}

/// Moves the top-level entries into `receive_dir` and returns where each one landed.
/// A fresh hidden directory below `receive_dir` for one download.
pub(super) fn new_stage(receive_dir: &Path, peer: MachineId, generation: u64) -> PathBuf {
    receive_dir
        .join(PARTIAL_DIR)
        .join(format!("{peer}-{generation}-{:016x}", OsRng.next_u64()))
}

pub(super) fn publish(
    stage: &Path,
    receive_dir: &Path,
    manifest: &ValidManifest,
) -> Result<Vec<PathBuf>> {
    create_directory(receive_dir).map_err(|_| Failure::Io)?;
    let mut published = Vec::new();
    for root in roots(manifest) {
        loop {
            let name = unique_name(root, |name| {
                fs::symlink_metadata(receive_dir.join(name)).is_ok()
            })
            .map_err(|_| Failure::Io)?;
            let target = receive_dir.join(name);
            match rename_no_replace(&stage.join(root), &target) {
                Ok(()) => {
                    published.push(target);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(Failure::Io),
            }
        }
    }
    fs::remove_dir(stage).map_err(|_| Failure::Io)?;
    Ok(published)
}

pub(in crate::daemon) fn cleanup(receive_dir: &Path, now: SystemTime) {
    let parent = receive_dir.join(PARTIAL_DIR);
    if a7n_platform::transfer_fs::check_directory(&parent).is_err() {
        return;
    }
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if !meta.is_dir() || meta.is_symlink() {
            continue;
        }
        if meta
            .modified()
            .ok()
            .and_then(|time| now.duration_since(time).ok())
            .is_some_and(|age| age > PARTIAL_MAX_AGE)
        {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}
