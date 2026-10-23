// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_proto::v1::{Envelope, envelope::Body};
use a7n_proto::{ALPN_LINK, ALPN_PAIR, MachineId, limits};
use quinn::Connection;
use tokio::sync::{OwnedSemaphorePermit, mpsc, oneshot};
use tokio::time::{Instant, timeout_at};

use super::{Daemon, DaemonError, Event};
use crate::command::Reply;
use crate::handshake;
use crate::link::{CLOSE_SUPERSEDED, Link, LinkError, LinkHandle, peer_certificate};
use crate::{identity, link_driver};

pub(super) struct PendingConnect {
    peer: MachineId,
    deadline: Instant,
    reply: Reply<()>,
}

impl Daemon {
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
                let events = self.events.clone();
                let info = self.info.clone();
                self.tasks.spawn(async move {
                    let result = timeout_at(
                        deadline,
                        handshake::respond(connection, &info, accepts_drive),
                    )
                    .await
                    .unwrap_or(Err(crate::link::LinkError::Timeout));
                    drop(permit);
                    let _ = events.send(Event::Link(result.map(Box::new), None)).await;
                });
                Ok(())
            }
            _ => {
                connection.close(0u32.into(), b"");
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
            Event::ClipboardOpened(stream, transfer_id, buf) => {
                self.clipboard_opened(stream, transfer_id, buf);
            }
            Event::ClipboardFetched {
                peer,
                link,
                request,
                data,
            } => self.clipboard_fetched(peer, link, request, data),
            Event::DiscoveryDialEnded(peer) => {
                self.discovery_dials.remove(&peer);
            }
            Event::Link(Err(LinkError::Superseded(peer)), Some(reply)) => {
                let deadline = Instant::now() + limits::HANDSHAKE_TIMEOUT;
                self.pending_connects.push(PendingConnect {
                    peer,
                    deadline,
                    reply,
                });
                self.finish_connects();
                let events = self.events.clone();
                self.tasks.spawn(async move {
                    tokio::time::sleep_until(deadline).await;
                    let _ = events.send(Event::ExpireConnects).await;
                });
            }
            Event::ExpireConnects => self.finish_connects(),
            Event::Link(result, reply) => {
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
            Event::PairingEnded => self.host_busy = false,
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
                let result = result.map_err(DaemonError::from).and_then(|entry| {
                    self.store_peer(entry.clone())?;
                    Ok(entry)
                });
                match result {
                    Ok(entry) => {
                        let (connected, receiver) = oneshot::channel();
                        self.connect(addr, connected);
                        self.tasks.spawn(async move {
                            if !matches!(receiver.await, Ok(Ok(()))) {
                                crate::log::event!("kind=post_pair_connect_failed");
                            }
                            let _ = reply.send(Ok(entry));
                        });
                    }
                    Err(error) => {
                        let _ = reply.send(Err(error));
                    }
                }
            }
            Event::Ended(peer, id) => {
                // An old task can finish after its replacement is registered.
                if self.is_current(peer, id) {
                    self.links.by_peer.remove(&peer);
                    self.clipboard.links.remove(&peer);
                    self.input_link_lost(peer);
                }
            }
        }
    }

    fn finish_connects(&mut self) {
        for pending in std::mem::take(&mut self.pending_connects) {
            let live = self.links.by_peer.get(&pending.peer).is_some_and(|handle| {
                handle.connection.close_reason().is_none() && !handle.sender.is_closed()
            });
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
            link.connection.close(0u32.into(), b"");
            return Err(crate::link::LinkError::InvalidHello.into());
        }
        self.pins
            .record_certificate(&peer_certificate(&link.connection)?)?;
        self.pins.save()?;
        let peer = link.peer_id;
        let id = link.connection.stable_id();
        let (sender, outbound) = mpsc::channel(super::MAILBOX_CAPACITY);
        let handle = LinkHandle {
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
                accepts: self.allow_drive(peer),
            })),
        });
        self.clipboard_link_up(peer);
        let transfers = self.transfer_tx.clone();
        let inbound = self.inbound.clone();
        let events = self.events.clone();
        self.tasks.spawn(async move {
            let connection = link.connection.clone();
            let _ = link_driver::drive(link, inbound, outbound, transfers).await;
            connection.close(0u32.into(), b"");
            let _ = events.send(Event::Ended(peer, id)).await;
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr};

    use tokio::sync::oneshot;

    use super::*;

    #[tokio::test]
    async fn finish_connects_superseded_without_winning_link_returns_error() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let mut daemon =
            Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let peer = daemon.identity.machine_id;
        let (reply, mut receiver) = oneshot::channel();
        daemon.pending_connects.push(PendingConnect {
            peer,
            deadline: Instant::now(),
            reply,
        });

        // Act
        daemon.event(Event::ExpireConnects);

        // Assert
        assert!(
            matches!(receiver.try_recv().unwrap(), Err(DaemonError::Link(LinkError::Superseded(id))) if id == peer)
        );
        assert!(daemon.pending_connects.is_empty());
    }

    #[tokio::test]
    async fn event_joined_follow_up_connect_fails_returns_stored_peer() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let remote_dir = tempfile::tempdir().unwrap();
        let mut daemon =
            Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let remote = identity::load_or_create(remote_dir.path()).unwrap();
        let entry = crate::pin_store::Entry {
            machine_id: remote.machine_id,
            pin: remote.pin,
            name: "paired peer".into(),
            spki_der: None,
        };
        let (reply, mut receiver) = oneshot::channel();

        // Act
        daemon.event(Event::Joined {
            result: Ok(entry.clone()),
            addr: SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
            reply,
        });
        assert!(matches!(
            receiver.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        let failed = daemon.event_rx.recv().await.unwrap();
        assert!(matches!(&failed, Event::Link(Err(_), Some(_))));
        daemon.event(failed);
        let result = receiver.await.unwrap();

        // Assert
        assert_eq!(result.unwrap(), entry);
        let pins = crate::pin_store::PinStore::load(&dir.path().join("pins.toml")).unwrap();
        assert_eq!(pins.get(&entry.machine_id), Some(&entry));
        assert!(!daemon.links.by_peer.contains_key(&entry.machine_id));
    }
}
