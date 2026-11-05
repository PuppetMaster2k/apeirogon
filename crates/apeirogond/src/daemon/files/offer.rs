// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use a7n_core::clipboard;
use a7n_core::files::ValidManifest;
use a7n_proto::limits;
use a7n_proto::v1::{
    ClipboardAnnounce, ClipboardItem, FileEntry, FileKind, FileManifest, envelope::Body,
};
use prost::Message;
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use super::{Daemon, DaemonError, Event, MachineId, OFFERS_PER_PEER};
use crate::command::Reply;
use crate::daemon::transfer::TransferError;

pub(in crate::daemon) struct Offer {
    pub wire: ClipboardAnnounce,
    pub manifest: ValidManifest,
    pub paths: Vec<PathBuf>,
    pub skipped: usize,
}

impl Daemon {
    pub(in crate::daemon) fn offer_files(
        &mut self,
        peer: MachineId,
        paths: Vec<PathBuf>,
        reply: Reply<(u64, usize)>,
    ) {
        let result = self.check_file_send(peer).and_then(|()| {
            if self
                .files
                .offers
                .keys()
                .filter(|(id, _)| *id == peer)
                .count()
                + self.files.building.get(&peer).copied().unwrap_or(0)
                >= OFFERS_PER_PEER
            {
                return Err(DaemonError::InvalidRequest("too many file offers"));
            }
            self.files.next_offer = self
                .files
                .next_offer
                .checked_add(1)
                .ok_or(DaemonError::InvalidRequest("offer ids exhausted"))?;
            let Some(link) = self.links.by_peer.get(&peer) else {
                return Err(DaemonError::NotConnected);
            };
            Ok((self.files.next_offer, link.connection.stable_id()))
        });
        let (offer, link) = match result {
            Ok(value) => value,
            Err(error) => {
                let _ = reply.send(Err(error));
                return;
            }
        };
        *self.files.building.entry(peer).or_default() += 1;
        let origin = self.identity.machine_id;
        let events = self.events.clone();
        self.tasks.spawn(async move {
            let result = tokio::task::spawn_blocking(move || build(origin, offer, paths))
                .await
                .unwrap_or(Err(TransferError::OfferWorker));
            let _ = events
                .send(Event::FilesBuilt {
                    peer,
                    link,
                    result,
                    reply,
                })
                .await;
        });
    }

    pub(in crate::daemon) fn files_built(
        &mut self,
        peer: MachineId,
        link: usize,
        result: Result<Offer, TransferError>,
        reply: Reply<(u64, usize)>,
    ) {
        if let Some(count) = self.files.building.get_mut(&peer) {
            *count = count.saturating_sub(1);
        }
        let result = result
            .map_err(|error| DaemonError::InvalidRequest(error.reason()))
            .and_then(|offer| {
                self.check_file_send(peer)?;
                if !self.is_current(peer, link) {
                    return Err(DaemonError::NotConnected);
                }
                let id = offer.wire.generation;
                let skipped = offer.skipped;
                self.send_input(peer, Body::ClipboardAnnounce(offer.wire.clone()));
                if !self.is_current(peer, link) {
                    return Err(DaemonError::NotConnected);
                }
                self.files.offers.insert((peer, id), Arc::new(offer));
                Ok((id, skipped))
            });
        let _ = reply.send(result);
    }
}

pub(super) fn build(
    origin: MachineId,
    generation: u64,
    paths: Vec<PathBuf>,
) -> Result<Offer, TransferError> {
    let mut manifest = FileManifest::default();
    let mut sources = Vec::new();
    let mut skipped = 0;
    for path in paths {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(TransferError::SourceName)?;
        walk(&path, name, &mut manifest, &mut sources, &mut skipped)?;
    }
    if manifest.entries.is_empty() {
        return Err(TransferError::EmptyOffer);
    }
    let valid = ValidManifest::try_from(&manifest).map_err(|_| TransferError::Manifest)?;
    let wire = ClipboardAnnounce {
        origin: origin.as_bytes().to_vec(),
        generation,
        send: true,
        items: vec![ClipboardItem {
            mime: clipboard::FILES.into(),
            size: manifest.total_size,
            ..ClipboardItem::default()
        }],
        files: Some(manifest),
    };
    if !fits_control_frame(&wire) {
        return Err(TransferError::ManifestTooLarge);
    }
    Ok(Offer {
        wire,
        manifest: valid,
        paths: sources,
        skipped,
    })
}

/// An announce, manifest included, has to fit in one bounded control envelope.
pub(in crate::daemon) fn fits_control_frame(wire: &ClipboardAnnounce) -> bool {
    (a7n_proto::v1::Envelope {
        body: Some(Body::ClipboardAnnounce(wire.clone())),
    })
    .encoded_len()
        <= limits::CONTROL_FRAME_MAX
}

fn walk(
    path: &Path,
    name: &str,
    manifest: &mut FileManifest,
    sources: &mut Vec<PathBuf>,
    skipped: &mut usize,
) -> Result<(), TransferError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| TransferError::SourceMetadata)?;
    if metadata.is_symlink() {
        *skipped += 1;
        crate::log::event!("kind=file_source_skipped reason=symlink");
        return Ok(());
    }
    if manifest.entries.len() >= limits::MANIFEST_ENTRIES_MAX {
        return Err(TransferError::TooManyEntries);
    }
    let name: String = name.nfc().collect();
    a7n_core::files::ManifestPath::try_from(name.as_str())
        .map_err(|_| TransferError::SourceName)?;
    let (kind, size, sha256) = if metadata.is_dir() {
        (FileKind::Directory, 0, Vec::new())
    } else if metadata.is_file() {
        let mut file = a7n_platform::transfer_fs::open_file(path, false, false)
            .map_err(|_| TransferError::SourceOpen)?;
        let mut hash = Sha256::new();
        super::staging::hash_file(&mut file, &mut hash).map_err(|_| TransferError::SourceHash)?;
        let length = file
            .metadata()
            .map_err(|_| TransferError::SourceMetadata)?
            .len();
        if length != metadata.len() {
            return Err(TransferError::SourceChanged);
        }
        (FileKind::File, length, hash.finalize().to_vec())
    } else {
        return Err(TransferError::SourceKind);
    };
    manifest.total_size = manifest
        .total_size
        .checked_add(size)
        .ok_or(TransferError::SizesOverflow)?;
    manifest.entries.push(FileEntry {
        path: name.clone(),
        kind: kind.into(),
        size,
        sha256,
        mtime_ms: 0,
    });
    sources.push(path.to_owned());
    if metadata.is_dir() {
        a7n_platform::transfer_fs::check_directory(path)
            .map_err(|_| TransferError::SourceDirectoryUnsafe)?;
        let mut children = fs::read_dir(path)
            .map_err(|_| TransferError::SourceDirectory)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| TransferError::SourceDirectory)?;
        children.sort_by_key(std::fs::DirEntry::file_name);
        for child in children {
            let child_name = child.file_name();
            let child_name = child_name.to_str().ok_or(TransferError::SourceName)?;
            walk(
                &child.path(),
                &format!("{name}/{child_name}"),
                manifest,
                sources,
                skipped,
            )?;
        }
    }
    Ok(())
}
