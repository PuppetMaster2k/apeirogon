// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use a7n_core::clipboard;
use a7n_core::files::ValidManifest;
use a7n_ipc::TransferDirection;
use a7n_ipc::config::ClipboardPolicy;
use a7n_proto::v1::{
    ClipboardAnnounce, ClipboardRef, FileResume, TransferRequest, envelope::Body,
    transfer_request::Source,
};
use tokio::sync::{oneshot, watch};

use super::notifications::Notice;
use super::{
    ASK_TIMEOUT, Active, Daemon, DaemonError, Failure, Incoming, Kind, MachineId, OFFERS_PER_PEER,
    Progress,
};

/// What one receive task needs; the caller decides how its result is used.
pub(super) struct Download {
    pub peer: MachineId,
    pub offer: u64,
    pub kind: Kind,
    pub manifest: ValidManifest,
    pub stage: PathBuf,
    pub receive_dir: PathBuf,
    pub total_size: u64,
}

impl Daemon {
    /// Starts a receive no user waits on, so a refusal only needs a log line.
    pub(in crate::daemon) fn log_receive_start(&mut self, peer: MachineId, offer: u64) {
        if self.start_file_receive(peer, offer).is_err() {
            crate::log::event!("kind=file_receive_not_started");
        }
    }

    pub(in crate::daemon) fn receive_file_offer(
        &mut self,
        peer: MachineId,
        wire: &ClipboardAnnounce,
    ) {
        if !self.files_capable(peer) || wire.origin != peer.as_bytes() || !wire.send {
            return;
        }
        let policy = self.files_policy(peer);
        let ask = policy == ClipboardPolicy::Ask;
        if !policy.receives() {
            crate::log::event!("kind=file_offer_ignored");
            return;
        }
        let Some(manifest) = wire.files.as_ref() else {
            return;
        };
        if wire.items.len() != 1
            || wire.items[0].mime != clipboard::FILES
            || wire.items[0].size != manifest.total_size
            || !wire.items[0].inline_data.is_empty()
            || !wire.items[0].sha256.is_empty()
            || manifest.entries.is_empty()
        {
            crate::log::event!("kind=file_offer_rejected");
            return;
        }
        let Ok(valid) = ValidManifest::try_from(manifest) else {
            crate::log::event!("kind=file_offer_rejected");
            return;
        };
        let key = (peer, wire.generation);
        if let Some(incoming) = self.files.incoming.get_mut(&key) {
            if incoming.wire != *wire {
                return;
            }
            incoming.announced_link = self.links.by_peer[&peer].connection.stable_id();
            if !incoming.active && incoming.deadline.is_none() {
                self.log_receive_start(peer, wire.generation);
            }
            return;
        }
        if ask {
            self.drop_held_offers(peer);
        }
        if self
            .files
            .incoming
            .keys()
            .filter(|(id, _)| *id == peer)
            .count()
            >= OFFERS_PER_PEER
        {
            return;
        }
        let Ok(receive_dir) = self.config.files.receive_dir() else {
            return;
        };
        let staging = super::staging::new_stage(&receive_dir, peer, wire.generation);
        if ask {
            self.notify_files(
                peer,
                &Progress::new(&valid, manifest.total_size),
                Notice::Offered,
            );
        }
        self.files.incoming.insert(
            key,
            Incoming {
                wire: wire.clone(),
                manifest: valid,
                staging,
                receive_dir,
                deadline: ask.then(|| Instant::now() + ASK_TIMEOUT),
                active: false,
                announced_link: self.links.by_peer[&peer].connection.stable_id(),
            },
        );
        if !ask {
            self.log_receive_start(peer, wire.generation);
        }
    }

    pub(in crate::daemon) fn accept_files_with_reply(
        &mut self,
        peer: MachineId,
        offer_id: u64,
        reply: crate::command::Reply<()>,
    ) {
        match self.accept_files(peer, offer_id) {
            Ok(offer) => {
                self.files.accepts.insert((peer, offer), reply);
            }
            Err(error) => {
                let _ = reply.send(Err(error));
            }
        }
    }

    pub(in crate::daemon) fn accept_files(
        &mut self,
        peer: MachineId,
        offer_id: u64,
    ) -> Result<u64, DaemonError> {
        self.expire_files(Instant::now());
        let key = (peer, offer_id);
        if self
            .files
            .incoming
            .get(&key)
            .is_none_or(|incoming| incoming.deadline.is_none())
        {
            return Err(DaemonError::InvalidRequest(
                "file offer changed; review it again",
            ));
        }
        if !self.links.by_peer.contains_key(&peer) {
            return Err(DaemonError::NotConnected);
        }
        if !self.files_policy(peer).receives() {
            return Err(DaemonError::InvalidRequest("file receiving is disabled"));
        }
        self.files
            .incoming
            .get_mut(&key)
            .expect("held offer exists")
            .deadline = None;
        self.start_file_receive(peer, key.1)?;
        Ok(key.1)
    }

    pub(in crate::daemon) fn reject_files(&mut self, peer: MachineId) -> Result<(), DaemonError> {
        let before = self.files.incoming.len();
        self.drop_held_offers(peer);
        if self.files.incoming.len() == before {
            return Err(DaemonError::InvalidRequest(
                "no file offer waiting for confirmation",
            ));
        }
        Ok(())
    }

    pub(super) fn start_file_receive(
        &mut self,
        peer: MachineId,
        offer: u64,
    ) -> Result<(), DaemonError> {
        if !self.files_capable(peer) {
            return Err(DaemonError::NotConnected);
        }
        if !self.files_policy(peer).receives() {
            return Err(DaemonError::InvalidRequest("file receiving is disabled"));
        }
        let incoming = self
            .files
            .incoming
            .get(&(peer, offer))
            .ok_or(DaemonError::InvalidRequest("unknown file offer"))?;
        if incoming.active {
            return Ok(());
        }
        let download = Download {
            peer,
            offer,
            kind: Kind::Offer,
            manifest: incoming.manifest.clone(),
            stage: incoming.staging.clone(),
            receive_dir: incoming.receive_dir.clone(),
            total_size: incoming.wire.items[0].size,
        };
        if self.spawn_receive(download)?.is_some() {
            if let Some(incoming) = self.files.incoming.get_mut(&(peer, offer)) {
                incoming.active = true;
            }
        }
        Ok(())
    }

    /// Starts the download task unless every transfer slot of the link is taken.
    /// Returns the id of the started transfer.
    pub(super) fn spawn_receive(&mut self, download: Download) -> Result<Option<u64>, DaemonError> {
        let Download {
            peer,
            offer,
            kind,
            manifest,
            stage,
            receive_dir,
            total_size,
        } = download;
        let transfers = self
            .clipboard
            .links
            .get_mut(&peer)
            .ok_or(DaemonError::NotConnected)?;
        let Ok(permit) = transfers.slots.clone().try_acquire_owned() else {
            return Ok(None);
        };
        transfers.next = transfers
            .next
            .checked_add(2)
            .ok_or(DaemonError::InvalidRequest("transfer ids exhausted"))?;
        self.files.next_transfer = self
            .files
            .next_transfer
            .checked_add(1)
            .ok_or(DaemonError::InvalidRequest("transfer ids exhausted"))?;
        let id = self.files.next_transfer;
        let wire_id = transfers.next;
        let link = transfers.id;
        let (sender, receiver) = oneshot::channel();
        transfers.pending.insert(wire_id, sender);
        let (cancel, cancelled) = watch::channel(Failure::Cancelled);
        let progress = Arc::new(Progress::new(&manifest, total_size));
        self.notify_files(peer, &progress, Notice::Receiving);
        self.files.active.insert(
            id,
            Active {
                peer,
                link,
                wire_id,
                offer,
                kind,
                direction: TransferDirection::Receive,
                cancel,
                progress: progress.clone(),
            },
        );
        let events = self.events.clone();
        let connection = self.links.by_peer[&peer].connection.clone();
        let reserved = self.files.reserved.clone();
        self.tasks.spawn(async move {
            let runtime = tokio::runtime::Handle::current();
            let worker_events = events.clone();
            let result = tokio::task::spawn_blocking(move || {
                runtime.block_on(super::receive::run(super::receive::Receive {
                    id,
                    manifest,
                    stage,
                    destination: receive_dir,
                    receiver,
                    cancelled,
                    progress,
                    events: worker_events,
                    connection,
                    reserved,
                }))
            })
            .await
            .unwrap_or(Err(Failure::Io));
            drop(permit);
            let _ = events
                .send(super::Event::FilesFinished { id, result })
                .await;
        });
        Ok(Some(id))
    }

    pub(in crate::daemon) fn files_ready(&mut self, id: u64, resume: Vec<FileResume>) {
        let Some(active) = self.files.active.get(&id) else {
            return;
        };
        let (peer, offer, wire_id, link) = (active.peer, active.offer, active.wire_id, active.link);
        let (send, item_index) = match active.kind {
            Kind::Offer => (true, 0),
            Kind::ClipboardPaste { item_index } => (false, item_index),
            Kind::ClipboardSend => return,
        };
        if !self.is_current(peer, link) {
            return;
        }
        if send {
            if let Some(reply) = self.files.accepts.remove(&(peer, offer)) {
                let _ = reply.send(Ok(()));
            }
        }
        self.send_input(
            peer,
            Body::TransferRequest(TransferRequest {
                transfer_id: wire_id,
                source: Some(Source::Clipboard(ClipboardRef {
                    origin: peer.as_bytes().to_vec(),
                    generation: offer,
                    send,
                })),
                item_index,
                resume,
            }),
        );
    }
}
