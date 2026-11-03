// SPDX-License-Identifier: GPL-3.0-or-later

//! Pasting files copied on a peer: the paste downloads them into the receive
//! directory and answers with where they landed.

use std::path::PathBuf;

use a7n_core::clipboard::{FILES, Generation};
use a7n_core::files::ValidManifest;
use a7n_ipc::TransferDirection;
use a7n_ipc::config::ClipboardPolicy;
use a7n_platform::clipboard::{PasteRequest, uri_list};
use a7n_proto::MachineId;
use a7n_proto::v1::{Capability, ClipboardAnnounce, FileManifest, TransferCancel, envelope::Body};

use super::incoming::Download;
use super::staging::roots;
use super::{Active, Daemon, Failure, Kind, Result};
use crate::daemon::clipboard::fetch::reject;

/// Downloads at least this large are logged when they start; the GUI and
/// `a7n status` show their progress through the transfer list.
const LARGE_PASTE_BYTES: u64 = 16 * 1024 * 1024;

/// How many top-level entries a paste of this manifest publishes; 0 when the
/// manifest is invalid.
pub(in crate::daemon) fn files_top_level(manifest: &FileManifest) -> u32 {
    ValidManifest::try_from(manifest)
        .ok()
        .and_then(|valid| u32::try_from(roots(&valid).len()).ok())
        .unwrap_or(0)
}

/// The last completed paste.
pub(super) struct Pasted {
    generation: Generation,
    paths: Vec<PathBuf>,
}

impl Daemon {
    /// A paste is an explicit local action, so the `ask` policy does not hold it.
    pub(in crate::daemon) fn clipboard_can_receive_files(&self, peer: MachineId) -> bool {
        self.clipboard
            .handle
            .as_ref()
            .is_some_and(|handle| handle.files)
            && matches!(
                self.files_policy(peer),
                ClipboardPolicy::Receive | ClipboardPolicy::Both | ClipboardPolicy::Ask
            )
            && self
                .links
                .by_peer
                .get(&peer)
                .is_some_and(|link| link.capabilities.contains(&Capability::ClipboardFiles))
    }

    /// The index of the file list in an announce this machine can paste, if it
    /// is the first one and its manifest is valid.
    pub(in crate::daemon) fn announced_files_index(
        &self,
        origin: MachineId,
        wire: &ClipboardAnnounce,
    ) -> Option<u32> {
        if !self.clipboard_can_receive_files(origin) {
            return None;
        }
        let index = wire.items.iter().position(|item| item.mime == FILES)?;
        let manifest = wire.files.as_ref()?;
        ValidManifest::try_from(manifest).ok()?;
        if wire.items[index].size != manifest.total_size {
            return None;
        }
        u32::try_from(index).ok()
    }

    pub(in crate::daemon) fn paste_files(&mut self, request: PasteRequest) {
        let peer = request.generation.origin;
        let announced = self.clipboard.announce.clone();
        let Some(wire) = announced
            .filter(|wire| self.announced_files_index(peer, wire) == Some(request.item_index))
        else {
            reject(request, "unavailable");
            return;
        };
        if let Some(pasted) = self.files.pasted.as_ref().filter(|pasted| {
            pasted.generation == request.generation
                && pasted
                    .paths
                    .iter()
                    .all(|path| std::fs::symlink_metadata(path).is_ok())
        }) {
            let _ = request.reply.send(Some(uri_list::encode(&pasted.paths)));
            return;
        }
        if let Some(waiting) = self.files.active.iter().find_map(|(id, active)| {
            (active.peer == peer
                && active.offer == request.generation.generation
                && active.kind
                    == Kind::ClipboardPaste {
                        item_index: request.item_index,
                    })
            .then_some(*id)
        }) {
            self.files
                .pastes
                .entry(waiting)
                .or_default()
                .push(request.reply);
            return;
        }
        let manifest = wire
            .files
            .as_ref()
            .and_then(|manifest| ValidManifest::try_from(manifest).ok());
        let receive_dir = self.config.files.receive_dir().ok();
        let (Some(manifest), Some(receive_dir)) = (manifest, receive_dir) else {
            reject(request, "unavailable");
            return;
        };
        let total_size = wire.items[request.item_index as usize].size;
        let download = Download {
            peer,
            offer: request.generation.generation,
            kind: Kind::ClipboardPaste {
                item_index: request.item_index,
            },
            manifest,
            stage: super::staging::new_stage(&receive_dir, peer, request.generation.generation),
            receive_dir,
            total_size,
        };
        match self.spawn_receive(download) {
            Ok(Some(id)) => {
                if total_size >= LARGE_PASTE_BYTES {
                    crate::log::event!("kind=files_paste_large bytes={total_size}");
                }
                self.files.pastes.insert(id, vec![request.reply]);
            }
            Ok(None) => reject(request, "busy"),
            Err(_) => reject(request, "link_lost"),
        }
    }

    /// Answers every paste that waited for this download.
    pub(super) fn finish_paste(
        &mut self,
        id: u64,
        active: &Active,
        result: Result<()>,
        published: Vec<PathBuf>,
    ) {
        debug_assert_eq!(active.direction, TransferDirection::Receive);
        let generation = Generation {
            generation: active.offer,
            origin: active.peer,
        };
        let replies = self.files.pastes.remove(&id).unwrap_or_default();
        // A newer copy replaced the one that was pasted; the files stay in the
        // receive directory, but the paste no longer applies.
        let answer = (result.is_ok() && self.clipboard.current == Some(generation))
            .then(|| uri_list::encode(&published));
        if answer.is_some() {
            self.files.pasted = Some(Pasted {
                generation,
                paths: published,
            });
        }
        if result.is_err()
            && result != Err(Failure::LinkLost)
            && self.is_current(active.peer, active.link)
        {
            self.send_input(
                active.peer,
                Body::TransferCancel(TransferCancel {
                    transfer_id: active.wire_id,
                }),
            );
        }
        for reply in replies {
            let _ = reply.send(answer.clone());
        }
    }
}
