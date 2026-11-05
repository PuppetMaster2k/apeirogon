// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::future::pending;
use std::sync::Arc;
use std::time::{Duration, Instant};

use a7n_core::clipboard::{self, Clock, Exclusion, Generation, LocalChange};
use a7n_ipc::config::ClipboardPolicy;
use a7n_ipc::{ClipboardCopy, ClipboardInfo, PeerClipboard};
use a7n_platform::clipboard::{Clipboard, ClipboardCommand, LazyItem, PasteRequest};
use a7n_proto::v1::{Capability, ClipboardAnnounce, envelope::Body};
use a7n_proto::{MachineId, limits};
use tokio::sync::{Semaphore, oneshot};

use super::Daemon;
use super::files::files_top_level;

mod ask;

// A local UX limit keeps unattended confirmations from lingering; this is not a wire limit.
const CLIPBOARD_ASK_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) struct Held {
    peer: MachineId,
    wire: ClipboardAnnounce,
    pub deadline: Instant,
}

pub(super) mod fetch;

pub(super) struct Transfers {
    pub(in crate::daemon) id: usize,
    pub(in crate::daemon) next: u64,
    pub(in crate::daemon) pending: HashMap<u64, oneshot::Sender<(quinn::RecvStream, Vec<u8>)>>,
    /// Sends and fetches share the per-link transfer limit (PROTOCOL.md 14).
    pub(in crate::daemon) slots: Arc<Semaphore>,
    read_slots: Arc<Semaphore>,
    send_tasks: HashMap<u64, tokio::task::AbortHandle>,
    fetch_tasks: HashMap<u64, tokio::task::AbortHandle>,
}

impl Transfers {
    fn new(id: usize, next: u64) -> Self {
        Self {
            id,
            next,
            pending: HashMap::new(),
            slots: Arc::new(Semaphore::new(limits::CONCURRENT_TRANSFERS_PER_LINK)),
            send_tasks: HashMap::new(),
            fetch_tasks: HashMap::new(),
            read_slots: Arc::new(Semaphore::new(limits::CONCURRENT_TRANSFERS_PER_LINK)),
        }
    }
}

#[derive(Default)]
pub(super) struct State {
    pub handle: Option<Clipboard>,
    pub held: Option<Held>,
    clock: Clock,
    pub(in crate::daemon) current: Option<Generation>,
    pub(in crate::daemon) announce: Option<ClipboardAnnounce>,
    own_items: Vec<clipboard::Item>,
    pub links: HashMap<MachineId, Transfers>,
}

pub(super) enum Event {
    Changed(Option<LocalChange>),
    Paste(PasteRequest),
}

pub(super) async fn next(handle: &mut Option<Clipboard>) -> Event {
    match handle {
        Some(handle) => tokio::select! {
            change = handle.changes.recv() => Event::Changed(change),
            Some(request) = handle.pastes.recv() => Event::Paste(request),
        },
        None => pending().await,
    }
}

impl Daemon {
    pub(super) fn clipboard_status(&self) -> ClipboardInfo {
        let current = self
            .clipboard
            .current
            .zip(self.clipboard.announce.as_ref())
            .and_then(|(generation, announce)| {
                Some(ClipboardCopy {
                    origin: generation.origin,
                    origin_name: if generation.origin == self.identity.machine_id {
                        Some(self.info.name.clone())
                    } else {
                        self.peer_name(generation.origin).map(str::to_owned)
                    },
                    generation: generation.generation,
                    items: u32::try_from(announce.items.len()).ok()?,
                    size: announce.items.iter().map(|item| item.size).sum(),
                    local: generation.origin == self.identity.machine_id,
                })
            });
        ClipboardInfo {
            available: self.clipboard.handle.is_some(),
            current,
            held: self.held_clipboard(),
            peers: self
                .peer_infos()
                .into_iter()
                .map(|peer| PeerClipboard {
                    machine: peer.id,
                    name: peer.name,
                    policy: self.clipboard_policy(peer.id),
                    linked: peer.connected,
                })
                .collect(),
        }
    }

    pub(crate) fn with_clipboard(mut self, handle: Option<Clipboard>) -> Self {
        if handle.as_ref().is_some_and(|handle| handle.images) {
            self.info
                .capabilities
                .push(Capability::ClipboardImage.into());
        }
        if handle.as_ref().is_some_and(|handle| handle.files) {
            self.info
                .capabilities
                .push(Capability::ClipboardFiles.into());
        }
        if handle.is_some() {
            self.info
                .capabilities
                .push(Capability::ClipboardText.into());
        }
        self.clipboard.handle = handle;
        self
    }

    fn clipboard_policy(&self, peer: MachineId) -> ClipboardPolicy {
        self.config
            .peer(&peer)
            .map_or(ClipboardPolicy::Both, |policy| policy.clipboard)
    }

    pub(super) fn clipboard_can_send(&self, peer: MachineId) -> bool {
        self.clipboard.handle.is_some()
            && self.clipboard_policy(peer).sends()
            && self
                .links
                .by_peer
                .get(&peer)
                .is_some_and(|link| link.capabilities.contains(&Capability::ClipboardText))
    }

    fn clipboard_images(&self) -> bool {
        self.clipboard
            .handle
            .as_ref()
            .is_some_and(|handle| handle.images)
    }

    fn clipboard_can_send_image(&self, peer: MachineId) -> bool {
        self.clipboard_images()
            && self
                .links
                .by_peer
                .get(&peer)
                .is_some_and(|link| link.capabilities.contains(&Capability::ClipboardImage))
    }

    /// What this peer is allowed to see of an announce. The files item is
    /// appended last, so dropping other items never changes its meaning.
    fn clipboard_wire_for(
        &self,
        peer: MachineId,
        mut wire: ClipboardAnnounce,
    ) -> ClipboardAnnounce {
        if !self.clipboard_can_send_image(peer) {
            wire.items.retain(|item| item.mime != clipboard::IMAGE_PNG);
        }
        if !self.clipboard_can_send_files(peer) {
            wire.items.retain(|item| item.mime != clipboard::FILES);
            wire.files = None;
        }
        wire
    }

    fn clipboard_announce_to(&mut self, peer: MachineId, wire: ClipboardAnnounce) {
        let wire = self.clipboard_wire_for(peer, wire);
        if !wire.items.is_empty() {
            self.send_input(peer, Body::ClipboardAnnounce(wire));
        }
    }

    /// Whether `item_index` of what `peer` was shown is this copy's file list.
    pub(super) fn clipboard_files_item(&self, peer: MachineId, item_index: u32) -> bool {
        let Some(wire) = self.clipboard.announce.clone() else {
            return false;
        };
        self.clipboard_wire_for(peer, wire)
            .items
            .get(item_index as usize)
            .is_some_and(|item| item.mime == clipboard::FILES)
    }

    /// Announces the copy to every peer allowed to see it and keeps it as the current one.
    pub(super) fn clipboard_publish(&mut self, wire: ClipboardAnnounce) {
        let peers: Vec<_> = self
            .links
            .by_peer
            .keys()
            .copied()
            .filter(|peer| self.clipboard_can_send(*peer))
            .collect();
        for peer in peers {
            self.clipboard_announce_to(peer, wire.clone());
        }
        self.clipboard.announce = Some(wire);
    }

    pub(super) fn clipboard_link_up(&mut self, peer: MachineId) {
        let Some(link) = self.links.by_peer.get(&peer) else {
            return;
        };
        let id = link.connection.stable_id();
        // Disjoint requester ids make cancellation unambiguous in both directions.
        let next = u64::from(self.identity.machine_id > peer);
        self.clipboard.links.insert(peer, Transfers::new(id, next));
        if !self.clipboard_can_send(peer) {
            return;
        }
        if let Some(wire) = self
            .clipboard
            .announce
            .as_ref()
            .filter(|wire| wire.origin == self.identity.machine_id.as_bytes())
            .cloned()
        {
            self.clipboard_announce_to(peer, wire);
        }
    }

    pub(super) fn clipboard_changed(&mut self, change: Option<LocalChange>) {
        self.clipboard.drop_held();
        self.clipboard.own_items.clear();
        self.files_clipboard_superseded();
        let Some(change) = change else {
            self.clipboard.handle = None;
            self.clipboard.announce = None;
            crate::log::event!("kind=clipboard_unavailable");
            return;
        };
        if !Exclusion::allows(&change, &self.config.clipboard.deny_apps) {
            self.clipboard.announce = None;
            crate::log::event!("kind=clipboard_excluded");
            return;
        }
        let generation = self.clipboard.clock.next(self.identity.machine_id);
        // The previous copy is no longer available, even if this copy has no supported formats.
        self.clipboard.announce = None;
        let files = self.clipboard_files_to_offer(change.files);
        // A file copy may have no other format, but still needs a generation.
        let wire = clipboard::announce(change.items.clone(), generation).or_else(|| {
            (!files.is_empty()).then(|| ClipboardAnnounce {
                origin: generation.origin.as_bytes().to_vec(),
                generation: generation.generation,
                ..ClipboardAnnounce::default()
            })
        });
        let Some(wire) = wire else {
            return;
        };
        self.clipboard.own_items = change
            .items
            .into_iter()
            .filter(|item| {
                !item.data.is_empty()
                    && wire
                        .items
                        .iter()
                        .any(|announced| announced.mime == item.mime)
            })
            .collect();
        self.clipboard.current = Some(generation);
        if files.is_empty() {
            self.clipboard_publish(wire);
        } else {
            // The manifest hashes every file, so peers learn of this copy only
            // once it is built: a same-generation re-announce would be ignored
            // as not newer, and a newer generation would lose the paste.
            self.clipboard_files_build(generation, wire, files);
        }
    }

    pub(super) fn receive_clipboard(&mut self, peer: MachineId, wire: &ClipboardAnnounce) {
        // Links carry only the sender's own clipboard (no relaying), so a
        // foreign origin is a spoof that would also hide that machine's copies.
        if wire.origin != peer.as_bytes() {
            crate::log::event!("kind=clipboard_rejected reason=origin");
            return;
        }
        // Observe the counter even for disabled policies or malformed payloads.
        self.clipboard.clock.observe(&Generation {
            generation: wire.generation,
            origin: self.identity.machine_id,
        });
        if self.clipboard.handle.is_none() || !self.clipboard_policy(peer).receives() {
            return;
        }
        if self.clipboard_policy(peer) == ClipboardPolicy::Ask {
            self.hold_clipboard(peer, wire);
            return;
        }
        self.apply_clipboard(wire);
    }

    fn apply_clipboard(&mut self, wire: &ClipboardAnnounce) {
        let mut received = match clipboard::accept(self.clipboard.current, wire) {
            Ok(Some(received)) => received,
            Ok(None) => return,
            Err(_) => {
                crate::log::event!("kind=clipboard_rejected");
                return;
            }
        };
        let images = self.clipboard_images();
        received
            .items
            .retain(|item| item.mime != clipboard::IMAGE_PNG || images);
        let files_index = self.announced_files_index(received.generation.origin, wire);
        received
            .pending
            .retain(|&index| match wire.items[index as usize].mime.as_str() {
                clipboard::IMAGE_PNG => images,
                clipboard::FILES => files_index == Some(index),
                _ => true,
            });
        if received.items.iter().any(|item| {
            item.mime == clipboard::IMAGE_PNG && clipboard::validate_png(&item.data).is_err()
        }) {
            return;
        }
        self.clipboard.drop_held();
        self.files_clipboard_superseded();
        self.clipboard.current = Some(received.generation);
        self.clipboard.announce = Some(wire.clone());
        self.clipboard.own_items.clear();
        let lazy: Vec<_> = received
            .pending
            .iter()
            .map(|&item_index| {
                let item = &wire.items[item_index as usize];
                LazyItem {
                    mime: item.mime.clone(),
                    size: item.size,
                    item_index,
                    top_level: if item.mime == clipboard::FILES {
                        wire.files.as_ref().map_or(0, files_top_level)
                    } else {
                        0
                    },
                }
            })
            .collect();
        // ponytail: delayed paste backends only; add eager fetch when a backend requires it.
        if received.items.is_empty() && lazy.is_empty() {
            return;
        }
        if self.clipboard.handle.as_ref().is_some_and(|handle| {
            handle
                .commands
                .try_send(ClipboardCommand::Set {
                    generation: received.generation,
                    items: received.items,
                    lazy,
                })
                .is_err()
        }) {
            self.clipboard.handle = None;
            self.clipboard.announce = None;
            crate::log::event!("kind=clipboard_unavailable");
        }
    }
}

#[cfg(test)]
mod tests;
