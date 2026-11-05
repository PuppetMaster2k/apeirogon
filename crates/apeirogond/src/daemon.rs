// SPDX-License-Identifier: GPL-3.0-or-later

//! One task owns identity, trust, desk membership, pairing and the live registry.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use a7n_core::pairing::{HostPairing, Paired, PairingCode};
use a7n_ipc::config::Config;
use a7n_proto::v1::{Envelope, PairDone, PeerInfo, envelope::Body};
use a7n_proto::{MachineId, limits};
use quinn::Endpoint;
use rand_core::OsRng;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinSet;

use self::transfer::TransferError;
#[cfg(test)]
use crate::command::Command;
use crate::command::Reply;
use crate::desk_store::DeskStore;
use crate::endpoint;
use crate::identity::{self, Identity};
use crate::layout_store::LayoutStore;
use crate::link::{Link, LinkError, Links};
use crate::pairing::PairingError;
use crate::pin_store::{Entry, PinStore};
use crate::settings_store::SettingsStore;
use crate::tls::{self, TrustSnapshot, TrustedPins};

mod clipboard;
mod command;
mod config;
mod connections;
mod error;
mod files;
mod input;
mod layout;
mod nearby;
mod pairing;
#[cfg(test)]
#[path = "pairing/tests.rs"]
mod pairing_tests;
mod run;
mod settings;
mod sync;
#[cfg(test)]
pub(crate) mod test_peer;
#[cfg(test)]
mod tests;
mod transfer;
mod wol;

/// Bounded mailboxes; a stalled consumer cannot grow memory without limit.
const MAILBOX_CAPACITY: usize = 64;

pub(crate) use error::DaemonError;
pub(crate) use input::Retried as InputRetried;

type TransferStream = (MachineId, usize, quinn::RecvStream);

type Inbound = (MachineId, usize, Envelope);

enum Event {
    Link(Result<Box<Link>, LinkError>, Option<Reply<()>>),
    Ended(MachineId, usize),
    PairingEnded,
    FilesBuilt {
        peer: MachineId,
        link: usize,
        result: Result<files::Offer, TransferError>,
        reply: Reply<(u64, usize)>,
    },
    FilesReady {
        id: u64,
        resume: Vec<a7n_proto::v1::FileResume>,
    },
    FilesFinished {
        id: u64,
        result: Result<Vec<std::path::PathBuf>, files::Failure>,
    },
    ClipboardFilesBuilt {
        generation: a7n_core::clipboard::Generation,
        wire: a7n_proto::v1::ClipboardAnnounce,
        result: Result<files::Offer, TransferError>,
    },
    ClipboardOpened(TransferStream, u64, Vec<u8>),
    ClipboardFetched {
        peer: MachineId,
        link: usize,
        request: a7n_platform::clipboard::PasteRequest,
        data: Option<Vec<u8>>,
    },
    ExpireConnects,
    DiscoveryDialEnded(MachineId),
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

struct Mailboxes {
    events: mpsc::Sender<Event>,
    event_rx: mpsc::Receiver<Event>,
    inbound: mpsc::Sender<Inbound>,
    inbound_rx: mpsc::Receiver<Inbound>,
    transfer_tx: mpsc::Sender<TransferStream>,
    transfer_rx: mpsc::Receiver<TransferStream>,
}

impl Mailboxes {
    fn new() -> Self {
        let (events, event_rx) = mpsc::channel(MAILBOX_CAPACITY);
        let (inbound, inbound_rx) = mpsc::channel(MAILBOX_CAPACITY);
        let (transfer_tx, transfer_rx) = mpsc::channel(MAILBOX_CAPACITY);
        Self {
            events,
            event_rx,
            inbound,
            inbound_rx,
            transfer_tx,
            transfer_rx,
        }
    }
}

pub(crate) struct Daemon {
    identity: Identity,
    pins: PinStore,
    desk: DeskStore,
    layout: LayoutStore,
    settings: SettingsStore,
    endpoint: Endpoint,
    info: PeerInfo,
    trust: TrustedPins,
    links: Links,
    pending_connects: Vec<connections::PendingConnect>,
    nearby: nearby::State,
    pairing: pairing::State,
    tasks: JoinSet<()>,
    mailboxes: Mailboxes,
    input: input::Input,
    clipboard: clipboard::State,
    files: files::State,
    sync: sync::State,
    wol: wol::State,
    config: Config,
    config_path: PathBuf,
    shutdown_reply: Option<Reply<()>>,
}

impl Daemon {
    pub(crate) fn load(path: &Path, addr: SocketAddr) -> Result<Self, DaemonError> {
        std::fs::create_dir_all(path)?;
        let identity = identity::load_or_create(path)?;
        let mut pins = PinStore::load(&path.join("pins.toml"))?;
        let desk = DeskStore::load(&path.join("desk.pb"), identity.machine_id, &mut pins)?;
        let layout = LayoutStore::load(&path.join("layout.pb"), identity.machine_id);
        let trust = Arc::new(RwLock::new(TrustSnapshot {
            pins: pins.pins().collect(),
            pairing_until: None,
        }));
        let config = tls::server_config(&identity, Arc::clone(&trust))?;
        // Keep wildcard listeners on IPv4, as on platforms without dual-stack sockets.
        let addr = if addr.ip().is_unspecified() {
            SocketAddr::from((Ipv4Addr::UNSPECIFIED, addr.port()))
        } else {
            addr
        };
        let mut endpoint = endpoint::bind_addr(addr, config)?;
        endpoint.set_default_client_config(tls::client_config(&identity, Arc::clone(&trust))?);
        let info = PeerInfo {
            platform: a7n_platform::platform().into(),
            name: identity.machine_id.to_string(),
            capabilities: vec![
                a7n_proto::v1::Capability::SettingsSync.into(),
                a7n_proto::v1::Capability::Files.into(),
            ],
            listen_port: u32::from(endpoint.local_addr()?.port()),
            ..PeerInfo::default()
        };
        let input = input::Input::new(identity.machine_id);
        Ok(Self {
            identity,
            pins,
            desk,
            layout,
            settings: SettingsStore::load(&path.join("settings.pb")),
            endpoint,
            info,
            trust,
            links: Links::default(),
            pending_connects: Vec::new(),
            nearby: nearby::State::default(),
            pairing: pairing::State::default(),
            tasks: JoinSet::new(),
            mailboxes: Mailboxes::new(),
            input,
            clipboard: clipboard::State::default(),
            files: files::State::default(),
            sync: sync::State::default(),
            wol: wol::State::default(),
            config: Config::default(),
            config_path: path.join("config.toml"),
            shutdown_reply: None,
        })
    }

    fn open_pairing(&mut self) -> Result<PairingCode, DaemonError> {
        if self.pairing.host_busy {
            return Err(DaemonError::PairingBusy);
        }
        let code = PairingCode::generate(&mut OsRng);
        let now = Instant::now();
        self.pairing.host = Some(HostPairing::new(code.clone(), None, self.identity.pin, now));
        self.trust
            .write()
            .map_err(|_| DaemonError::TrustPoisoned)?
            .pairing_until = Some(now + limits::PAIRING_CODE_DURATION);
        Ok(code)
    }

    fn connect(&mut self, addr: SocketAddr, reply: Reply<()>) {
        let endpoint = self.endpoint.clone();
        let info = self.info.clone();
        let events = self.mailboxes.events.clone();
        self.tasks.spawn(async move {
            let result = endpoint::dial(&endpoint, addr, &info).await.map(Box::new);
            let _ = events.send(Event::Link(result, Some(reply))).await;
        });
    }

    fn unpair(&mut self, machine: MachineId) -> Result<(), DaemonError> {
        let result = (|| {
            if machine == self.identity.machine_id || self.pins.get(&machine).is_none() {
                return Err(DaemonError::NotPaired);
            }
            self.desk
                .revoke(&self.identity, &mut self.pins, machine, unix_seconds()?)?;
            Ok(())
        })();
        // Revocation remains effective in memory even when a pin write fails.
        self.sync_trust()?;
        self.drop_untrusted();
        result?;
        self.broadcast(&Envelope {
            body: Some(Body::Desk(self.desk.to_desk())),
        });
        Ok(())
    }

    fn peer_name(&self, peer: MachineId) -> Option<&str> {
        self.pins.get(&peer).map(|entry| entry.name.as_str())
    }

    fn sync_trust(&self) -> Result<(), DaemonError> {
        self.trust
            .write()
            .map_err(|_| DaemonError::TrustPoisoned)?
            .pins = self.pins.pins().collect();
        Ok(())
    }

    fn receive(&mut self, envelope: Envelope) -> Result<(), DaemonError> {
        if let Some(Body::Layout(layout)) = &envelope.body {
            return self.receive_layout(layout);
        }
        if let Some(Body::Desk(desk)) = envelope.body {
            let before = self.desk.to_desk();
            let result = self.desk.merge(&self.identity, &mut self.pins, &desk);
            // Revocation remains effective in memory even when a pin write fails.
            self.sync_trust()?;
            self.drop_untrusted();
            let outcome = result?;
            if outcome.self_revoked {
                crate::log::event!("kind=self_revoked");
            }
            if before != self.desk.to_desk() {
                self.broadcast(&Envelope {
                    body: Some(Body::Desk(self.desk.to_desk())),
                });
            }
        }
        Ok(())
    }

    fn broadcast(&mut self, envelope: &Envelope) {
        let mut lost = Vec::new();
        self.links.by_peer.retain(|peer, handle| {
            if matches!(envelope.body, Some(Body::Settings(_)))
                && !handle
                    .capabilities
                    .contains(&a7n_proto::v1::Capability::SettingsSync)
            {
                return true;
            }
            if handle.sender.try_send(envelope.clone()).is_ok() {
                return true;
            }
            // A slow peer catches up from persistent state on its next handshake.
            handle.connection.close(crate::link::CLOSE_NORMAL, b"");
            lost.push(*peer);
            false
        });
        for peer in lost {
            self.input_link_lost(peer);
        }
    }

    fn drop_untrusted(&mut self) {
        let mut lost = Vec::new();
        self.links.by_peer.retain(|peer, handle| {
            if self.pins.get(peer).is_some() {
                return true;
            }
            handle.connection.close(crate::link::CLOSE_NORMAL, b"");
            lost.push(*peer);
            false
        });
        for peer in lost {
            self.input_link_lost(peer);
        }
    }

    fn peer_message_failed(&mut self, peer: MachineId) {
        if let Some(handle) = self.links.by_peer.remove(&peer) {
            handle.connection.close(crate::link::CLOSE_NORMAL, b"");
        }
        self.input_link_lost(peer);
        crate::log::event!("kind=peer_message_failed");
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
        self.endpoint.close(crate::link::CLOSE_NORMAL, b"");
    }
}

fn unix_seconds() -> Result<u64, DaemonError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}
