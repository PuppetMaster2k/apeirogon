// SPDX-License-Identifier: GPL-3.0-or-later

//! One task owns identity, trust, desk membership, pairing and the live registry.

use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use a7n_core::pairing::{AttemptLimiter, HostPairing, Paired, PairingCode};
use a7n_proto::v1::{Envelope, PairDone, PeerInfo, envelope::Body};
use a7n_proto::{MachineId, limits};
use quinn::Endpoint;
use rand_core::OsRng;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinSet;

use crate::command::{Command, Reply, Status};
use crate::desk_store::{DeskStore, DeskStoreError};
use crate::endpoint;
use crate::identity::{self, Identity, IdentityError};
use crate::link::{Link, LinkError, Links};
use crate::pairing::PairingError;
use crate::pin_store::{Entry, PinStore, PinStoreError};
use crate::tls::{self, TlsError, TrustSnapshot, TrustedPins};

mod connections;
mod pairing;
#[cfg(test)]
#[path = "pairing/tests.rs"]
mod pairing_tests;
#[cfg(test)]
pub(crate) mod test_peer;
#[cfg(test)]
mod tests;

/// Bounded mailboxes; a stalled consumer cannot grow memory without limit.
const MAILBOX_CAPACITY: usize = 64;
/// Bound the number of source-address rate-limit buckets.
const PAIRING_SOURCES: usize = 256;

type Inbound = (MachineId, usize, Envelope);

#[derive(Debug, thiserror::Error)]
pub(crate) enum DaemonError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Identity(#[from] IdentityError),
    #[error(transparent)]
    Pins(#[from] PinStoreError),
    #[error(transparent)]
    Desk(#[from] DeskStoreError),
    #[error(transparent)]
    Tls(#[from] TlsError),
    #[error(transparent)]
    Link(#[from] LinkError),
    #[error(transparent)]
    Pairing(#[from] PairingError),
    #[error("system time precedes the Unix epoch")]
    Clock(#[from] std::time::SystemTimeError),
    #[error("pairing exchange already active")]
    PairingBusy,
    #[error("machine is not paired")]
    NotPaired,
    #[error("trust snapshot lock poisoned")]
    TrustPoisoned,
}

enum Event {
    Link(Result<Box<Link>, LinkError>, Option<Reply<()>>),
    Ended(MachineId, usize),
    PairingEnded,
    ExpireConnects,
    Confirmed {
        entry: Entry,
        paired: Paired,
        reply: oneshot::Sender<Result<PairDone, PairingError>>,
    },
    Joined {
        result: Result<Entry, PairingError>,
        addr: SocketAddr,
        reply: Reply<Entry>,
    },
}

pub(crate) struct Daemon {
    identity: Identity,
    pins: PinStore,
    desk: DeskStore,
    endpoint: Endpoint,
    info: PeerInfo,
    trust: TrustedPins,
    links: Links,
    pending_connects: Vec<connections::PendingConnect>,
    host: Option<HostPairing>,
    host_busy: bool,
    limiter: AttemptLimiter<IpAddr>,
    tasks: JoinSet<()>,
    events: mpsc::Sender<Event>,
    event_rx: mpsc::Receiver<Event>,
    inbound: mpsc::Sender<Inbound>,
    inbound_rx: mpsc::Receiver<Inbound>,
}

impl Daemon {
    pub(crate) fn load(path: &Path, addr: SocketAddr) -> Result<Self, DaemonError> {
        std::fs::create_dir_all(path)?;
        let identity = identity::load_or_create(path)?;
        let mut pins = PinStore::load(&path.join("pins.toml"))?;
        let desk = DeskStore::load(&path.join("desk.pb"), identity.machine_id, &mut pins)?;
        let trust = Arc::new(RwLock::new(TrustSnapshot {
            pins: pins.pins().collect(),
            pairing_until: None,
        }));
        let config = tls::server_config(&identity, Arc::clone(&trust))?;
        let mut endpoint = if addr.ip().is_unspecified() {
            endpoint::bind(addr.port(), config)?
        } else {
            endpoint::bind_addr(addr, config)?
        };
        endpoint.set_default_client_config(tls::client_config(&identity, Arc::clone(&trust))?);
        let info = PeerInfo {
            name: crate::pin_store::hex_encode(identity.machine_id.as_bytes()),
            listen_port: u32::from(endpoint.local_addr()?.port()),
            ..PeerInfo::default()
        };
        let (events, event_rx) = mpsc::channel(MAILBOX_CAPACITY);
        let (inbound, inbound_rx) = mpsc::channel(MAILBOX_CAPACITY);
        Ok(Self {
            identity,
            pins,
            desk,
            endpoint,
            info,
            trust,
            links: Links::default(),
            pending_connects: Vec::new(),
            host: None,
            host_busy: false,
            limiter: AttemptLimiter::new(PAIRING_SOURCES),
            tasks: JoinSet::new(),
            events,
            event_rx,
            inbound,
            inbound_rx,
        })
    }

    pub(crate) async fn run(
        mut self,
        mut commands: mpsc::Receiver<Command>,
    ) -> Result<(), DaemonError> {
        let (accepted, mut accepted_rx) = mpsc::channel(MAILBOX_CAPACITY);
        self.tasks
            .spawn(endpoint::accept_loop(self.endpoint.clone(), accepted));
        loop {
            tokio::select! {
                command = commands.recv() => {
                    let Some(command) = command else { return Ok(()); };
                    self.command(command)?;
                }
                Some((connection, permit, deadline)) = accepted_rx.recv() => {
                    self.accept(connection, permit, deadline)?;
                }
                Some(event) = self.event_rx.recv() => self.event(event),
                Some((peer, id, envelope)) = self.inbound_rx.recv() => {
                    if self.is_current(peer, id) && self.receive(envelope).is_err() {
                        if let Some(handle) = self.links.by_peer.remove(&peer) {
                            handle.connection.close(0u32.into(), b"");
                        }
                        eprintln!("kind=desk_failed");
                    }
                }
                Some(_) = self.tasks.join_next(), if !self.tasks.is_empty() => {}
            }
        }
    }

    fn command(&mut self, command: Command) -> Result<(), DaemonError> {
        match command {
            Command::Status(reply) => {
                let _ = reply.send(Ok(self.status()));
            }
            Command::PairHost(reply) => {
                let _ = reply.send(self.open_pairing());
            }
            Command::PairJoin { addr, code, reply } => {
                self.join(addr, code, reply);
            }
            Command::Unpair { machine, reply } => {
                let result = self.unpair(machine);
                self.sync_trust()?;
                self.drop_untrusted();
                let _ = reply.send(result);
            }
            Command::Connect { addr, reply } => {
                self.connect(addr, reply);
            }
        }
        Ok(())
    }

    fn status(&self) -> Status {
        Status {
            machine: self.identity.machine_id,
            name: self.info.name.clone(),
            peers: self
                .pins
                .machine_ids()
                .filter_map(|id| {
                    self.pins.get(&id).map(|entry| a7n_ipc::PeerInfo {
                        id,
                        name: entry.name.clone(),
                        connected: self.links.by_peer.get(&id).is_some_and(|handle| {
                            handle.connection.close_reason().is_none() && !handle.sender.is_closed()
                        }),
                        rtt_ms: None,
                    })
                })
                .collect(),
        }
    }

    fn open_pairing(&mut self) -> Result<PairingCode, DaemonError> {
        if self.host_busy {
            return Err(DaemonError::PairingBusy);
        }
        let code = PairingCode::generate(&mut OsRng);
        let now = Instant::now();
        self.host = Some(HostPairing::new(code.clone(), None, self.identity.pin, now));
        self.trust
            .write()
            .map_err(|_| DaemonError::TrustPoisoned)?
            .pairing_until = Some(now + limits::PAIRING_CODE_DURATION);
        Ok(code)
    }

    fn connect(&mut self, addr: SocketAddr, reply: Reply<()>) {
        let endpoint = self.endpoint.clone();
        let info = self.info.clone();
        let events = self.events.clone();
        self.tasks.spawn(async move {
            let result = endpoint::dial(&endpoint, addr, &info).await.map(Box::new);
            let _ = events.send(Event::Link(result, Some(reply))).await;
        });
    }

    fn unpair(&mut self, machine: MachineId) -> Result<(), DaemonError> {
        if machine == self.identity.machine_id || self.pins.get(&machine).is_none() {
            return Err(DaemonError::NotPaired);
        }
        self.desk
            .revoke(&self.identity, &mut self.pins, machine, unix_seconds()?)?;
        self.sync_trust()?;
        self.drop_untrusted();
        self.broadcast();
        Ok(())
    }

    fn sync_trust(&self) -> Result<(), DaemonError> {
        self.trust
            .write()
            .map_err(|_| DaemonError::TrustPoisoned)?
            .pins = self.pins.pins().collect();
        Ok(())
    }

    fn receive(&mut self, envelope: Envelope) -> Result<(), DaemonError> {
        if let Some(Body::Desk(desk)) = envelope.body {
            let before = self.desk.to_desk();
            let result = self.desk.merge(&self.identity, &mut self.pins, &desk);
            // Revocation remains effective in memory even when a pin write fails.
            self.sync_trust()?;
            self.drop_untrusted();
            let outcome = result?;
            if outcome.self_revoked {
                eprintln!("kind=self_revoked");
            }
            if before != self.desk.to_desk() {
                self.broadcast();
            }
        }
        Ok(())
    }

    fn broadcast(&mut self) {
        let envelope = Envelope {
            body: Some(Body::Desk(self.desk.to_desk())),
        };
        self.links.by_peer.retain(|_, handle| {
            if handle.sender.try_send(envelope.clone()).is_ok() {
                return true;
            }
            // A slow peer catches up from persistent Desk on its next handshake.
            handle.connection.close(0u32.into(), b"");
            false
        });
    }

    fn drop_untrusted(&mut self) {
        self.links.by_peer.retain(|peer, handle| {
            if self.pins.get(peer).is_some() {
                return true;
            }
            handle.connection.close(0u32.into(), b"");
            false
        });
    }

    fn is_current(&self, peer: MachineId, id: usize) -> bool {
        self.links
            .by_peer
            .get(&peer)
            .is_some_and(|handle| handle.connection.stable_id() == id)
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.endpoint.close(0u32.into(), b"");
    }
}

fn unix_seconds() -> Result<u64, DaemonError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}
