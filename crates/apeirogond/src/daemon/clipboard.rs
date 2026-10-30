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

mod ask;
pub(super) use ask::deadline;

// A local UX limit keeps unattended confirmations from lingering; this is not a wire limit.
const CLIPBOARD_ASK_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) struct Held {
    peer: MachineId,
    wire: ClipboardAnnounce,
    pub deadline: Instant,
}

mod fetch;

pub(super) struct Transfers {
    id: usize,
    next: u64,
    pending: HashMap<u64, oneshot::Sender<(quinn::RecvStream, Vec<u8>)>>,
    send_slots: Arc<Semaphore>,
    read_slots: Arc<Semaphore>,
    fetch_slots: Arc<Semaphore>,
}

impl Transfers {
    fn new(id: usize) -> Self {
        Self {
            id,
            next: 0,
            pending: HashMap::new(),
            send_slots: Arc::new(Semaphore::new(limits::CONCURRENT_TRANSFERS_PER_LINK)),
            read_slots: Arc::new(Semaphore::new(limits::CONCURRENT_TRANSFERS_PER_LINK)),
            fetch_slots: Arc::new(Semaphore::new(limits::CONCURRENT_TRANSFERS_PER_LINK)),
        }
    }
}

#[derive(Default)]
pub(super) struct State {
    pub handle: Option<Clipboard>,
    pub held: Option<Held>,
    clock: Clock,
    current: Option<Generation>,
    announce: Option<ClipboardAnnounce>,
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
                        self.pins
                            .get(&generation.origin)
                            .map(|entry| entry.name.clone())
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
                .status()
                .peers
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

    fn clipboard_can_send(&self, peer: MachineId) -> bool {
        self.clipboard.handle.is_some()
            && matches!(
                self.clipboard_policy(peer),
                ClipboardPolicy::Both | ClipboardPolicy::Send | ClipboardPolicy::Ask
            )
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

    fn clipboard_announce_to(&mut self, peer: MachineId, mut wire: ClipboardAnnounce) {
        if !self.clipboard_can_send_image(peer) {
            wire.items.retain(|item| item.mime != clipboard::IMAGE_PNG);
        }
        if !wire.items.is_empty() {
            self.send_input(peer, Body::ClipboardAnnounce(wire));
        }
    }

    pub(super) fn clipboard_link_up(&mut self, peer: MachineId) {
        let id = self.links.by_peer[&peer].connection.stable_id();
        self.clipboard.links.insert(peer, Transfers::new(id));
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
        let Some(wire) = clipboard::announce(change.items.clone(), generation) else {
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

    pub(super) fn receive_clipboard(&mut self, peer: MachineId, wire: &ClipboardAnnounce) {
        // Observe the counter even for disabled policies or malformed payloads.
        self.clipboard.clock.observe(&Generation {
            generation: wire.generation,
            origin: self.identity.machine_id,
        });
        if self.clipboard.handle.is_none()
            || !matches!(
                self.clipboard_policy(peer),
                ClipboardPolicy::Both | ClipboardPolicy::Receive | ClipboardPolicy::Ask
            )
        {
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
        received
            .pending
            .retain(|&index| images || wire.items[index as usize].mime != clipboard::IMAGE_PNG);
        if received.items.iter().any(|item| {
            item.mime == clipboard::IMAGE_PNG && clipboard::validate_png(&item.data).is_err()
        }) {
            return;
        }
        self.clipboard.drop_held();
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
