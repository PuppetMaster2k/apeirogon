// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_proto::v1::{Envelope, envelope::Body};
use a7n_proto::{ALPN_LINK, ALPN_PAIR, MachineId, limits};
use quinn::Connection;
use tokio::sync::{OwnedSemaphorePermit, mpsc};
use tokio::time::{Instant, timeout_at};

use super::{Daemon, DaemonError, Event};
use crate::command::Reply;
use crate::handshake;
use crate::link::{CLOSE_SUPERSEDED, Link, LinkError, LinkHandle, link_problem, peer_certificate};
use crate::{identity, link_driver};

pub(super) struct PendingConnect {
    peer: MachineId,
    deadline: Instant,
    reply: Reply<()>,
}

impl Daemon {
    pub(super) fn is_linked(&self, peer: MachineId) -> bool {
        self.links.by_peer.get(&peer).is_some_and(|handle| {
            handle.connection.close_reason().is_none() && !handle.sender.is_closed()
        })
    }

    pub(super) fn accept(
        &mut self,
        connection: Connection,
        permit: OwnedSemaphorePermit,
        deadline: Instant,
    ) -> Result<(), DaemonError> {
        let protocol = connection
            .handshake_data()
            .and_then(|data| data.downcast::<quinn::crypto::rustls::HandshakeData>().ok())
            .and_then(|data| data.protocol);
        match protocol.as_deref() {
            Some(ALPN_PAIR) => self.host_pairing(connection, permit),
            Some(ALPN_LINK) if self.trusted(&connection) => {
                let peer = identity::machine_id_of(&peer_certificate(&connection)?)?;
                let accepts_drive = self.allow_drive(peer);
                let events = self.mailboxes.events.clone();
                let info = handshake::local_info(&connection, &self.endpoint, &self.info);
                self.tasks.spawn(async move {
                    let result = timeout_at(
                        deadline,
                        handshake::respond(connection, &info, accepts_drive),
                    )
                    .await
                    .unwrap_or(Err(crate::link::LinkError::Timeout));
                    drop(permit);
                    let _ = events
                        .send(Event::Link(Some(peer), result.map(Box::new), None))
                        .await;
                });
                Ok(())
            }
            _ => {
                connection.close(crate::link::CLOSE_NORMAL, b"");
                Ok(())
            }
        }
    }

    fn trusted(&self, connection: &Connection) -> bool {
        let Ok(cert) = peer_certificate(connection) else {
            return false;
        };
        let Ok(id) = identity::machine_id_of(&cert) else {
            return false;
        };
        let Ok(pin) = identity::pin_of(&cert) else {
            return false;
        };
        self.pins.get(&id).is_some_and(|entry| entry.pin == pin)
    }

    pub(super) fn event(&mut self, event: Event) {
        match event {
            Event::DiscoveryDialEnded(peer) => {
                self.nearby.discovery_dials.remove(&peer);
            }
            Event::Link(_, Err(LinkError::Superseded(peer)), Some(reply)) => {
                self.wait_for_connect(peer, reply);
            }
            Event::ExpireConnects => self.finish_connects(),
            Event::FirewallChecked(result) => self.firewall_checked(result),
            Event::Link(peer, result, reply) => {
                self.link_received(peer, result, reply);
            }
            Event::PairingEnded => self.pairing.host_busy = false,
            Event::Confirmed {
                entry,
                paired,
                reply,
            } => {
                let _ = reply.send(self.admit(&entry, paired));
            }
            Event::Joined {
                result,
                addr,
                reply,
            } => {
                self.joined(result, addr, reply);
            }
            Event::Ended(peer, id, problem) => {
                // An old task can finish after its replacement is registered.
                if self.is_current(peer, id) {
                    self.link_problems.insert(peer, problem);
                    self.links.by_peer.remove(&peer);
                    self.input_link_lost(peer);
                }
            }
            event => self.transfer_event(event),
        }
    }

    fn wait_for_connect(&mut self, peer: MachineId, reply: Reply<()>) {
        let deadline = Instant::now() + limits::HANDSHAKE_TIMEOUT;
        self.pending_connects.push(PendingConnect {
            peer,
            deadline,
            reply,
        });
        self.finish_connects();
        let events = self.mailboxes.events.clone();
        self.tasks.spawn(async move {
            tokio::time::sleep_until(deadline).await;
            let _ = events.send(Event::ExpireConnects).await;
        });
    }

    fn link_received(
        &mut self,
        peer: Option<MachineId>,
        result: Result<Box<Link>, LinkError>,
        reply: Option<Reply<()>>,
    ) {
        if let (Some(peer), Err(error)) = (peer, &result)
            && self.pins.get(&peer).is_some()
            && !self.is_linked(peer)
            && !matches!(error, LinkError::Superseded(_))
        {
            self.link_problems.insert(peer, link_problem(error));
        }
        let result = result
            .map_err(DaemonError::from)
            .and_then(|link| self.register(*link));
        if let Some(reply) = reply {
            let _ = reply.send(result);
        } else if result.is_err() {
            crate::log::event!("kind=handshake_failed");
        }
        self.finish_connects();
    }

    fn transfer_event(&mut self, event: Event) {
        match event {
            Event::FilesBuilt {
                peer,
                link,
                result,
                reply,
            } => self.files_built(peer, link, result, reply),
            Event::FilesReady { id, resume } => self.files_ready(id, resume),
            Event::FilesFinished { id, result } => self.files_finished(id, result),
            Event::ClipboardFilesBuilt {
                generation,
                wire,
                result,
            } => self.clipboard_files_built(generation, wire, result),
            Event::ClipboardOpened(stream, transfer_id, buf) => {
                self.clipboard_opened(stream, transfer_id, buf);
            }
            Event::ClipboardFetched {
                peer,
                link,
                request,
                data,
            } => self.clipboard_fetched(peer, link, request, data),
            _ => unreachable!("only transfer events are dispatched here"),
        }
    }

    fn finish_connects(&mut self) {
        for pending in std::mem::take(&mut self.pending_connects) {
            let live = self.is_linked(pending.peer);
            if live {
                let _ = pending.reply.send(Ok(()));
            } else if Instant::now() >= pending.deadline {
                let _ = pending
                    .reply
                    .send(Err(LinkError::Superseded(pending.peer).into()));
            } else {
                self.pending_connects.push(pending);
            }
        }
    }

    pub(super) fn register(&mut self, link: Link) -> Result<(), DaemonError> {
        if !self.trusted(&link.connection) {
            link.connection.close(crate::link::CLOSE_NORMAL, b"");
            return Err(crate::link::LinkError::InvalidHello.into());
        }
        self.pins
            .record_certificate(&peer_certificate(&link.connection)?)?;
        self.pins
            .record_hw_addr(link.peer_id, &link.peer_info.hw_addr);
        self.pins
            .record_screen_mm(link.peer_id, link.peer_info.screen_mm.as_ref());
        self.pins.save()?;
        let peer = link.peer_id;
        let id = link.connection.stable_id();
        let (sender, outbound) = mpsc::channel(super::MAILBOX_CAPACITY);
        let handle = LinkHandle {
            platform: link.peer_info.platform(),
            capabilities: link.capabilities.clone(),
            sender: sender.clone(),
            connection: link.connection.clone(),
            role: link.role,
        };
        if let Some(loser) = self.links.insert(peer, handle, self.identity.machine_id) {
            let rejected = loser.connection.stable_id() == id;
            loser.connection.close(CLOSE_SUPERSEDED, b"");
            if rejected {
                return Ok(());
            }
            self.input_link_lost(peer);
        }
        self.peer_monitors.insert(
            peer,
            link.peer_info
                .monitors
                .iter()
                .map(a7n_ipc::Monitor::from)
                .collect(),
        );
        self.link_problems.remove(&peer);
        self.send_link_state(&link, &sender);
        self.clipboard_link_up(peer);
        self.files_link_up(peer);
        self.spawn_link_driver(link, outbound);
        Ok(())
    }

    fn send_link_state(&self, link: &Link, sender: &mpsc::Sender<Envelope>) {
        // Both sides send Desk immediately, before the driver handles input.
        let _ = sender.try_send(Envelope {
            body: Some(Body::Desk(self.desk.to_desk())),
        });
        let _ = sender.try_send(Envelope {
            body: Some(Body::Layout(self.layout.state.to_proto())),
        });
        if link
            .capabilities
            .contains(&a7n_proto::v1::Capability::SettingsSync)
        {
            if let Some(settings) = self.settings_message() {
                let _ = sender.try_send(settings);
            }
        }
        let _ = sender.try_send(Envelope {
            body: Some(Body::Accepts(a7n_proto::v1::Accepts {
                accepts: self.allow_drive(link.peer_id),
            })),
        });
    }

    fn spawn_link_driver(&mut self, link: Link, outbound: mpsc::Receiver<Envelope>) {
        let peer = link.peer_id;
        let id = link.connection.stable_id();
        let transfers = self.mailboxes.transfer_tx.clone();
        let inbound = self.mailboxes.inbound.clone();
        let events = self.mailboxes.events.clone();
        self.tasks.spawn(async move {
            let connection = link.connection.clone();
            let error = link_driver::drive(link, inbound, outbound, transfers)
                .await
                .map_or_else(|error| error, LinkError::Liveness);
            connection.close(crate::link::CLOSE_NORMAL, b"");
            let _ = events
                .send(Event::Ended(peer, id, link_problem(&error)))
                .await;
        });
    }
}
