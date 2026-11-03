// SPDX-License-Identifier: GPL-3.0-or-later

//! Copied files on this machine, offered to peers as part of the clipboard.

use std::path::PathBuf;
use std::sync::Arc;

use a7n_core::clipboard::Generation;
use a7n_ipc::config::ClipboardPolicy;
use a7n_proto::MachineId;
use a7n_proto::v1::{Capability, ClipboardAnnounce};

use super::{Daemon, Event, Offer, offer};

impl Daemon {
    /// Whether `peer` may be shown (and may fetch) the files of a local copy.
    pub(in crate::daemon) fn clipboard_can_send_files(&self, peer: MachineId) -> bool {
        self.clipboard_can_send(peer)
            && matches!(
                self.files_policy(peer),
                ClipboardPolicy::Send | ClipboardPolicy::Both | ClipboardPolicy::Ask
            )
            && self
                .links
                .by_peer
                .get(&peer)
                .is_some_and(|link| link.capabilities.contains(&Capability::ClipboardFiles))
    }

    /// The copied files worth hashing: none unless a linked peer could paste
    /// them, because building the manifest reads every file.
    pub(in crate::daemon) fn clipboard_files_to_offer(&self, files: Vec<PathBuf>) -> Vec<PathBuf> {
        let wanted = self
            .links
            .by_peer
            .keys()
            .any(|peer| self.clipboard_can_send_files(*peer));
        if wanted { files } else { Vec::new() }
    }

    /// Builds the manifest off the event loop; `clipboard_files_built` finishes the announce.
    pub(in crate::daemon) fn clipboard_files_build(
        &mut self,
        generation: Generation,
        wire: ClipboardAnnounce,
        paths: Vec<PathBuf>,
    ) {
        let events = self.events.clone();
        self.tasks.spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                offer::build(generation.origin, generation.generation, paths)
            })
            .await
            .unwrap_or(Err("file offer worker failed"));
            let _ = events
                .send(Event::ClipboardFilesBuilt {
                    generation,
                    wire,
                    result,
                })
                .await;
        });
    }

    pub(in crate::daemon) fn clipboard_files_built(
        &mut self,
        generation: Generation,
        mut wire: ClipboardAnnounce,
        result: Result<Offer, &'static str>,
    ) {
        // A newer copy, local or remote, replaced this one while it was hashed.
        if self.clipboard.current != Some(generation) {
            return;
        }
        match result {
            Ok(offer) => {
                let mut with_files = wire.clone();
                with_files.items.extend(offer.wire.items.iter().cloned());
                with_files.files.clone_from(&offer.wire.files);
                if offer::fits_control_frame(&with_files) {
                    wire = with_files;
                    self.files.clipboard_offer = Some((generation.generation, Arc::new(offer)));
                } else {
                    crate::log::event!("kind=clipboard_files_failed reason=announce_too_large");
                }
            }
            Err(reason) => crate::log::event!("kind=clipboard_files_failed reason={reason}"),
        }
        if !wire.items.is_empty() {
            self.clipboard_publish(wire);
        }
    }

    /// Drops the offer and the paste cache of the copy that was replaced.
    pub(in crate::daemon) fn files_clipboard_superseded(&mut self) {
        self.files.clipboard_offer = None;
        self.files.pasted = None;
    }

    /// The offer for what a peer asks of the current clipboard generation.
    pub(super) fn clipboard_files_offer(
        &self,
        peer: MachineId,
        generation: u64,
        item_index: u32,
    ) -> Option<Arc<Offer>> {
        let (offered, offer) = self.files.clipboard_offer.as_ref()?;
        let current = Generation {
            generation,
            origin: self.identity.machine_id,
        };
        (*offered == generation
            && self.clipboard.current == Some(current)
            && self.clipboard_files_item(peer, item_index))
        .then(|| Arc::clone(offer))
    }
}
