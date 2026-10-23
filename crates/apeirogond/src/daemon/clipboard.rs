// SPDX-License-Identifier: GPL-3.0-or-later

use std::future::pending;

use a7n_core::clipboard::{self, Clock, Exclusion, Generation, LocalChange};
use a7n_ipc::config::ClipboardPolicy;
use a7n_platform::clipboard::{Clipboard, ClipboardCommand};
use a7n_proto::MachineId;
use a7n_proto::v1::{Capability, ClipboardAnnounce, envelope::Body};

use super::Daemon;

#[derive(Default)]
pub(super) struct State {
    pub handle: Option<Clipboard>,
    clock: Clock,
    current: Option<Generation>,
    // Retain pending metadata for fetch-on-paste in #18 part 2.
    announce: Option<ClipboardAnnounce>,
}

pub(super) async fn changed(handle: &mut Option<Clipboard>) -> Option<LocalChange> {
    match handle {
        Some(handle) => handle.changes.recv().await,
        None => pending().await,
    }
}

impl Daemon {
    pub(crate) fn with_clipboard(mut self, handle: Option<Clipboard>) -> Self {
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
                ClipboardPolicy::Both | ClipboardPolicy::Send
            )
            && self
                .links
                .by_peer
                .get(&peer)
                .is_some_and(|link| link.capabilities.contains(&Capability::ClipboardText))
    }

    pub(super) fn clipboard_link_up(&mut self, peer: MachineId) {
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
            self.send_input(peer, Body::ClipboardAnnounce(wire));
        }
    }

    pub(super) fn clipboard_changed(&mut self, change: Option<LocalChange>) {
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
        // The previous copy is no longer available, even if this copy has no supported text.
        self.clipboard.announce = None;
        let Some(wire) = clipboard::announce(change.items, generation) else {
            return;
        };
        self.clipboard.current = Some(generation);
        let peers: Vec<_> = self
            .links
            .by_peer
            .keys()
            .copied()
            .filter(|peer| self.clipboard_can_send(*peer))
            .collect();
        for peer in peers {
            self.send_input(peer, Body::ClipboardAnnounce(wire.clone()));
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
                ClipboardPolicy::Both | ClipboardPolicy::Receive
            )
        {
            return;
        }
        let received = match clipboard::accept(self.clipboard.current, wire) {
            Ok(Some(received)) => received,
            Ok(None) => return,
            Err(_) => {
                crate::log::event!("kind=clipboard_rejected");
                return;
            }
        };
        self.clipboard.current = Some(received.generation);
        self.clipboard.announce = Some(wire.clone());
        if received.pending != 0 {
            crate::log::event!("kind=clipboard_fetch_pending");
            return;
        }
        if received.items.is_empty() {
            return;
        }
        if self.clipboard.handle.as_ref().is_some_and(|handle| {
            handle
                .commands
                .try_send(ClipboardCommand::Set {
                    generation: received.generation,
                    items: received.items,
                    lazy: Vec::new(),
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
