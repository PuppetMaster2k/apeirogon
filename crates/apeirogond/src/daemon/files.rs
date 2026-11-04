// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicU32, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

use a7n_core::files::ValidManifest;
use a7n_ipc::config::ClipboardPolicy;
use a7n_ipc::{FileTransfer, FinishedSend, HeldFiles, SendOutcome, TransferDirection};
use a7n_proto::MachineId;
use a7n_proto::v1::{Capability, ClipboardAnnounce, TransferCancel, envelope::Body};
use tokio::sync::{oneshot, watch};

use super::{Daemon, DaemonError, Event};

mod incoming;
mod offer;
mod paste;
mod receive;
mod serve;
mod source;
mod staging;

pub(super) use offer::Offer;
pub(in crate::daemon) use paste::files_top_level;
pub(super) use staging::cleanup;

const OFFERS_PER_PEER: usize = 4;
const ASK_TIMEOUT: Duration = Duration::from_secs(60);
/// Ended sends kept for clients to read; older ones are dropped.
const FINISHED_SENDS_KEPT: usize = 16;
/// Hidden directory of the receive directory that holds unfinished downloads.
const PARTIAL_DIR: &str = ".apeirogon-partial";
const PARTIAL_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Default)]
pub(super) struct State {
    next_offer: u64,
    next_transfer: u64,
    offers: HashMap<(MachineId, u64), Arc<Offer>>,
    building: HashMap<MachineId, usize>,
    incoming: HashMap<(MachineId, u64), Incoming>,
    active: HashMap<u64, Active>,
    /// Pastes waiting for the download with this active id, answered when it ends.
    pastes: HashMap<u64, Vec<oneshot::Sender<Option<Vec<u8>>>>>,
    accepts: HashMap<(MachineId, u64), crate::command::Reply<()>>,
    /// The copied files this machine serves for its current clipboard generation.
    clipboard_offer: Option<(u64, Arc<Offer>)>,
    /// The last pasted copy, so pasting again hands out the same paths.
    pasted: Option<paste::Pasted>,
    finished_sends: VecDeque<FinishedSend>,
}

struct Incoming {
    wire: ClipboardAnnounce,
    manifest: ValidManifest,
    staging: PathBuf,
    receive_dir: PathBuf,
    deadline: Option<Instant>,
    active: bool,
    announced_link: usize,
}

struct Active {
    peer: MachineId,
    link: usize,
    wire_id: u64,
    /// The offer id, or for clipboard kinds the clipboard generation.
    offer: u64,
    kind: Kind,
    direction: TransferDirection,
    cancel: watch::Sender<Failure>,
    progress: Arc<Progress>,
}

/// Which namespace a transfer lives in; `ClipboardRef.send` selects it on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// An explicit `a7n send` with its own offer ids.
    Offer,
    /// Served from this machine's current clipboard generation.
    ClipboardSend,
    /// Downloaded because something pasted the peer's copied files.
    ClipboardPaste { item_index: u32 },
}

#[derive(Default)]
struct Progress {
    files: AtomicU32,
    bytes: AtomicU64,
    files_total: u32,
    bytes_total: u64,
}

impl Progress {
    fn new(manifest: &ValidManifest, bytes_total: u64) -> Self {
        Self {
            files_total: u32::try_from(
                manifest
                    .entries()
                    .iter()
                    .filter(|entry| matches!(entry.kind, a7n_core::files::EntryKind::File { .. }))
                    .count(),
            )
            .unwrap_or(u32::MAX),
            bytes_total,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Failure {
    Cancelled,
    LinkLost,
    Integrity,
    Invalid,
    Io,
    NoSpace,
}

type Result<T> = std::result::Result<T, Failure>;

impl Daemon {
    pub(in crate::daemon) fn files_policy(&self, peer: MachineId) -> ClipboardPolicy {
        self.config
            .peer(&peer)
            .map_or(ClipboardPolicy::Ask, |policy| policy.files)
    }

    fn files_capable(&self, peer: MachineId) -> bool {
        self.links
            .by_peer
            .get(&peer)
            .is_some_and(|link| link.capabilities.contains(&Capability::Files))
    }

    fn check_file_send(&self, peer: MachineId) -> std::result::Result<(), DaemonError> {
        if !self.links.by_peer.contains_key(&peer) {
            return Err(DaemonError::NotConnected);
        }
        if !self.files_capable(peer) {
            return Err(DaemonError::InvalidRequest(
                "peer does not support file offers",
            ));
        }
        if !matches!(
            self.files_policy(peer),
            ClipboardPolicy::Send | ClipboardPolicy::Both | ClipboardPolicy::Ask
        ) {
            return Err(DaemonError::InvalidRequest("file sending is disabled"));
        }
        Ok(())
    }

    pub(in crate::daemon) fn file_transfers(&self) -> Vec<FileTransfer> {
        let mut result: Vec<_> = self
            .files
            .active
            .iter()
            .map(|(&id, active)| FileTransfer {
                id,
                offer_id: (active.kind == Kind::Offer
                    && active.direction == TransferDirection::Send)
                    .then_some(active.offer),
                peer: active.peer,
                direction: active.direction,
                files_done: active.progress.files.load(Ordering::Relaxed),
                files_total: active.progress.files_total,
                bytes_done: active.progress.bytes.load(Ordering::Relaxed),
                bytes_total: active.progress.bytes_total,
            })
            .collect();
        result.sort_by_key(|item| item.id);
        result
    }

    pub(in crate::daemon) fn finished_sends(&self) -> Vec<FinishedSend> {
        self.files.finished_sends.iter().cloned().collect()
    }

    fn record_finished_send(&mut self, active: &Active, result: Result<()>) {
        // Link loss keeps the offer live, so the receiver may resume it.
        let outcome = match result {
            Ok(()) => SendOutcome::Completed,
            Err(Failure::LinkLost) => return,
            Err(Failure::Cancelled) => SendOutcome::Cancelled,
            Err(Failure::Integrity | Failure::Invalid | Failure::Io | Failure::NoSpace) => {
                SendOutcome::Failed
            }
        };
        if self.files.finished_sends.len() == FINISHED_SENDS_KEPT {
            self.files.finished_sends.pop_front();
        }
        self.files.finished_sends.push_back(FinishedSend {
            offer_id: active.offer,
            peer: active.peer,
            outcome,
        });
    }

    pub(in crate::daemon) fn held_files(&self) -> Vec<HeldFiles> {
        self.files
            .incoming
            .iter()
            .filter(|(_, incoming)| {
                incoming
                    .deadline
                    .is_some_and(|deadline| deadline > Instant::now())
            })
            .map(|(&(peer, _), incoming)| HeldFiles {
                peer,
                files: Progress::new(&incoming.manifest, 0).files_total,
                top_level_entries: u32::try_from(staging::roots(&incoming.manifest).len())
                    .unwrap_or(u32::MAX),
                total_size: incoming.wire.items[0].size,
            })
            .collect()
    }

    pub(in crate::daemon) fn files_deadline(&self) -> Option<Instant> {
        self.files
            .incoming
            .values()
            .filter_map(|incoming| incoming.deadline)
            .min()
    }

    pub(in crate::daemon) fn expire_files(&mut self, now: Instant) {
        self.files
            .incoming
            .retain(|_, incoming| incoming.deadline.is_none_or(|deadline| now < deadline));
    }

    pub(in crate::daemon) fn files_link_up(&mut self, peer: MachineId) {
        if self.check_file_send(peer).is_ok() {
            let wires: Vec<_> = self
                .files
                .offers
                .iter()
                .filter(|((id, _), _)| *id == peer)
                .map(|(_, offer)| offer.wire.clone())
                .collect();
            for wire in wires {
                self.send_input(peer, Body::ClipboardAnnounce(wire));
            }
        }
    }

    pub(in crate::daemon) fn files_link_down(&mut self, peer: MachineId) {
        self.files.accepts.retain(|(id, _), _| *id != peer);
        for active in self
            .files
            .active
            .values()
            .filter(|active| active.peer == peer)
        {
            let _ = active.cancel.send(Failure::LinkLost);
        }
        self.files
            .incoming
            .retain(|(id, _), incoming| *id != peer || incoming.deadline.is_none());
        if self.pins.get(&peer).is_none() {
            self.files.offers.retain(|(id, _), _| *id != peer);
        }
    }

    pub(in crate::daemon) fn recheck_files_policy(&mut self) {
        let cancelled: Vec<_> = self
            .files
            .active
            .iter()
            .filter(|(_, active)| {
                let policy = self.files_policy(active.peer);
                match active.direction {
                    TransferDirection::Send => !matches!(
                        policy,
                        ClipboardPolicy::Send | ClipboardPolicy::Both | ClipboardPolicy::Ask
                    ),
                    TransferDirection::Receive => !matches!(
                        policy,
                        ClipboardPolicy::Receive | ClipboardPolicy::Both | ClipboardPolicy::Ask
                    ),
                }
            })
            .map(|(&id, _)| id)
            .collect();
        for id in cancelled {
            let _ = self.cancel_file_transfer(id);
        }
        let config = &self.config;
        self.files.offers.retain(|(peer, _), _| {
            matches!(
                config
                    .peer(peer)
                    .map_or(ClipboardPolicy::Ask, |policy| policy.files),
                ClipboardPolicy::Send | ClipboardPolicy::Both | ClipboardPolicy::Ask
            )
        });
        self.files.incoming.retain(|(peer, _), incoming| {
            incoming.active
                || matches!(
                    config
                        .peer(peer)
                        .map_or(ClipboardPolicy::Ask, |policy| policy.files),
                    ClipboardPolicy::Receive | ClipboardPolicy::Both | ClipboardPolicy::Ask
                )
        });
        let incoming = &self.files.incoming;
        self.files
            .accepts
            .retain(|key, _| incoming.contains_key(key));
    }

    pub(in crate::daemon) fn cancel_file_transfer(
        &mut self,
        id: u64,
    ) -> std::result::Result<(), DaemonError> {
        let active = self
            .files
            .active
            .get(&id)
            .ok_or(DaemonError::InvalidRequest("unknown transfer"))?;
        let peer = active.peer;
        let wire_id = active.wire_id;
        let _ = active.cancel.send(Failure::Cancelled);
        self.send_input(
            peer,
            Body::TransferCancel(TransferCancel {
                transfer_id: wire_id,
            }),
        );
        Ok(())
    }

    pub(in crate::daemon) fn files_cancel_from(&mut self, peer: MachineId, wire_id: u64) {
        for active in self
            .files
            .active
            .values()
            .filter(|active| active.peer == peer && active.wire_id == wire_id)
        {
            let _ = active.cancel.send(Failure::Cancelled);
        }
        self.clipboard_cancel(peer, wire_id);
    }

    pub(in crate::daemon) fn files_finished(
        &mut self,
        id: u64,
        outcome: std::result::Result<Vec<PathBuf>, Failure>,
    ) {
        let Some(active) = self.files.active.remove(&id) else {
            return;
        };
        let result = outcome.as_ref().map(|_| ()).map_err(|failure| *failure);
        if let Some(reply) = (active.kind == Kind::Offer
            && active.direction == TransferDirection::Receive)
            .then(|| self.files.accepts.remove(&(active.peer, active.offer)))
            .flatten()
        {
            let error = if result == Err(Failure::NoSpace) {
                DaemonError::NoSpace
            } else {
                DaemonError::InvalidRequest("file receive failed before starting")
            };
            let _ = reply.send(Err(error));
        }
        if let Some(transfers) = self
            .clipboard
            .links
            .get_mut(&active.peer)
            .filter(|transfers| transfers.id == active.link)
        {
            if active.direction == TransferDirection::Receive {
                transfers.pending.remove(&active.wire_id);
            }
        }
        match active.kind {
            Kind::Offer => self.finish_offer(&active, result),
            Kind::ClipboardSend => {}
            Kind::ClipboardPaste { .. } => {
                self.finish_paste(id, &active, result, outcome.unwrap_or_default());
            }
        }
        if let Err(reason) = result {
            let reason = match reason {
                Failure::NoSpace => {
                    if active.kind == Kind::Offer {
                        crate::log::event!("kind=file_offer_no_space");
                    }
                    "no_space"
                }
                Failure::Integrity => a7n_proto::v1::AbortReason::Integrity.as_str_name(),
                Failure::Io => a7n_proto::v1::AbortReason::IoError.as_str_name(),
                Failure::Invalid => a7n_proto::v1::AbortReason::Refused.as_str_name(),
                Failure::Cancelled | Failure::LinkLost => {
                    a7n_proto::v1::AbortReason::Cancelled.as_str_name()
                }
            };
            crate::log::event!("kind=file_transfer_failed reason={reason}");
        }
        self.start_waiting_files(active.peer);
    }

    fn finish_offer(&mut self, active: &Active, result: Result<()>) {
        if active.direction == TransferDirection::Send {
            if result != Err(Failure::LinkLost) {
                self.files.offers.remove(&(active.peer, active.offer));
            }
            self.record_finished_send(active, result);
        } else if result == Err(Failure::LinkLost) {
            if let Some(incoming) = self.files.incoming.get_mut(&(active.peer, active.offer)) {
                incoming.active = false;
                if incoming.announced_link == active.link {
                    incoming.announced_link = usize::MAX;
                }
            }
            // Reannouncement may have arrived while the previous worker was stopping.
            if self
                .links
                .by_peer
                .get(&active.peer)
                .is_some_and(|link| link.connection.stable_id() != active.link)
                && self
                    .files
                    .incoming
                    .get(&(active.peer, active.offer))
                    .is_some_and(|incoming| self.is_current(active.peer, incoming.announced_link))
            {
                let _ = self.start_file_receive(active.peer, active.offer);
            }
        } else {
            self.files.incoming.remove(&(active.peer, active.offer));
            if result.is_err() && self.is_current(active.peer, active.link) {
                self.send_input(
                    active.peer,
                    Body::TransferCancel(TransferCancel {
                        transfer_id: active.wire_id,
                    }),
                );
            }
        }
    }

    pub(in crate::daemon) fn start_waiting_files(&mut self, peer: MachineId) {
        let waiting: Vec<_> = self
            .files
            .incoming
            .iter()
            .filter(|((id, _), incoming)| {
                *id == peer
                    && !incoming.active
                    && incoming.deadline.is_none()
                    && self.is_current(peer, incoming.announced_link)
            })
            .map(|((_, offer), _)| *offer)
            .collect();
        for offer in waiting {
            let _ = self.start_file_receive(peer, offer);
        }
    }
}

#[cfg(test)]
mod tests;
