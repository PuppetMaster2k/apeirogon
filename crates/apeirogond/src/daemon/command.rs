// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Instant;

use super::{Daemon, DaemonError, wol};
use crate::command::{Command, Status};

impl Daemon {
    pub(super) async fn command(&mut self, command: Command) -> Result<(), DaemonError> {
        match command {
            Command::Shutdown(_) => unreachable!("shutdown is handled by the run loop"),
            Command::Switch { machine, reply } => {
                let result = self.switch_machine(machine).await.map(|()| {
                    self.wol
                        .pending
                        .as_ref()
                        .and_then(|pending| self.peer_name(pending.peer).map(str::to_owned))
                });
                let _ = reply.send(result);
            }
            Command::Wake { machine, reply } => {
                let _ = reply.send(self.wake_peer(machine, wol::WakeAction::Only, Instant::now()));
            }
            Command::SetPaused { paused, reply } => {
                self.set_paused(paused).await;
                let _ = reply.send(Ok(()));
            }
            Command::ReloadConfig(reply) => {
                let result = self.reload_config().await;
                let _ = reply.send(result);
            }
            Command::RecordHotkey { kind, reply } => self.record_hotkey(kind, reply),
            Command::Status(reply) => {
                let _ = reply.send(Ok(self.status()));
            }
            Command::SendFiles { peer, paths, reply } => self.offer_files(peer, paths, reply),
            Command::FilesAccept {
                peer,
                offer_id,
                reply,
            } => {
                self.accept_files_with_reply(peer, offer_id, reply);
            }
            Command::FilesReject { peer, reply } => {
                let _ = reply.send(self.reject_files(peer));
            }
            Command::CancelTransfer { id, reply } => {
                let _ = reply.send(self.cancel_file_transfer(id));
            }
            Command::ClipboardStatus(reply) => {
                let _ = reply.send(Ok(self.clipboard_status()));
            }
            Command::ClipboardAccept {
                machine,
                generation,
                reply,
            } => {
                let _ = reply.send(self.accept_clipboard(machine, generation));
            }
            Command::ClipboardReject(reply) => {
                let _ = reply.send(self.reject_clipboard());
            }
            Command::GetLayout(reply) => {
                let _ = reply.send(Ok(self.layout_info()));
            }
            Command::SetLayout { placed, reply } => {
                let _ = reply.send(self.set_layout(placed));
            }
            Command::PairHost(reply) => {
                let _ = reply.send(self.open_pairing());
            }
            Command::PairJoin { addr, code, reply } => {
                self.join(addr, code, reply);
            }
            Command::Unpair { machine, reply } => {
                let result = self.unpair(machine);
                if matches!(result, Err(DaemonError::TrustPoisoned)) {
                    return Err(DaemonError::TrustPoisoned);
                }
                let _ = reply.send(result);
            }
            Command::Connect { addr, reply } => {
                self.connect(addr, reply);
            }
            Command::RequestPermission { permission, reply } => {
                let _ =
                    reply.send(a7n_platform::permissions::request(permission).map_err(Into::into));
            }
        }
        Ok(())
    }

    pub(super) fn status(&self) -> Status {
        Status {
            input: self.input.unavailable.clone(),
            held_clipboard: self.held_clipboard(),
            transfers: self.file_transfers(),
            held_files: self.held_files(),
            finished_sends: self.finished_sends(),
            pid: std::process::id(),
            paused: self.input.paused,
            focused: match self.input.focus.cursor() {
                a7n_core::focus::Cursor::Home => self.identity.machine_id,
                a7n_core::focus::Cursor::Driving(peer) => peer,
            },
            machine: self.identity.machine_id,
            name: self.info.name.clone(),
            peers: self.peer_infos(),
            nearby: self.nearby_status(),
            missing_permissions: a7n_platform::permissions::missing()
                .into_iter()
                .map(Into::into)
                .collect(),
            firewall: self.firewall.problems.clone(),
        }
    }

    pub(super) fn peer_infos(&self) -> Vec<a7n_ipc::PeerInfo> {
        self.pins
            .machine_ids()
            .filter_map(|id| {
                self.pins.get(&id).map(|entry| a7n_ipc::PeerInfo {
                    id,
                    name: entry.name.clone(),
                    connected: self.is_linked(id),
                    rtt_ms: None,
                    can_wake: entry.hw_addr.is_some(),
                    waking: self.wol.pending.as_ref().is_some_and(|pending| {
                        pending.peer == id && pending.deadline > Instant::now()
                    }),
                })
            })
            .collect()
    }
}
