// SPDX-License-Identifier: GPL-3.0-or-later

//! One task owns identity, trust, desk membership, pairing and the live registry.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use a7n_core::layout::LayoutError;
use a7n_core::pairing::{AttemptLimiter, HostPairing, Paired, PairingCode};
use a7n_ipc::config::{Config, ConfigError};
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
use crate::layout_store::LayoutStore;
use crate::link::{Link, LinkError, Links};
use crate::pairing::PairingError;
use crate::pin_store::{Entry, PinStore, PinStoreError};
use crate::settings_store::SettingsStore;
use crate::tls::{self, TlsError, TrustSnapshot, TrustedPins};

mod clipboard;
mod config;
mod connections;
mod input;
mod layout;
mod nearby;
mod pairing;
#[cfg(test)]
#[path = "pairing/tests.rs"]
mod pairing_tests;
mod settings;
#[cfg(test)]
pub(crate) mod test_peer;
#[cfg(test)]
mod tests;

/// Bounded mailboxes; a stalled consumer cannot grow memory without limit.
const MAILBOX_CAPACITY: usize = 64;
/// Bound the number of source-address rate-limit buckets.
const PAIRING_SOURCES: usize = 256;

type TransferStream = (MachineId, usize, quinn::RecvStream);

type Inbound = (MachineId, usize, Envelope);

#[derive(Debug, thiserror::Error)]
pub(crate) enum DaemonError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Identity(#[from] IdentityError),
    #[error(transparent)]
    Pins(#[from] PinStoreError),
    #[error(transparent)]
    Desk(#[from] DeskStoreError),
    #[error(transparent)]
    Layout(#[from] LayoutError),
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
    #[error("input sharing is paused")]
    Paused,
    #[error("machine is not paired")]
    NotPaired,
    #[error("{0}")]
    InvalidRequest(&'static str),
    #[error("machine is not connected")]
    NotConnected,
    #[error("trust snapshot lock poisoned")]
    TrustPoisoned,
}

enum Event {
    Link(Result<Box<Link>, LinkError>, Option<Reply<()>>),
    Ended(MachineId, usize),
    PairingEnded,
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
    nearby: HashMap<MachineId, Vec<SocketAddr>>,
    discovery_dials: HashSet<MachineId>,
    host: Option<HostPairing>,
    host_busy: bool,
    limiter: AttemptLimiter<IpAddr>,
    tasks: JoinSet<()>,
    events: mpsc::Sender<Event>,
    event_rx: mpsc::Receiver<Event>,
    inbound: mpsc::Sender<Inbound>,
    inbound_rx: mpsc::Receiver<Inbound>,
    transfer_tx: mpsc::Sender<TransferStream>,
    transfer_rx: mpsc::Receiver<TransferStream>,
    input: input::Input,
    clipboard: clipboard::State,
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
        let mut endpoint = if addr.ip().is_unspecified() {
            endpoint::bind(addr.port(), config)?
        } else {
            endpoint::bind_addr(addr, config)?
        };
        endpoint.set_default_client_config(tls::client_config(&identity, Arc::clone(&trust))?);
        let info = PeerInfo {
            name: identity.machine_id.to_string(),
            capabilities: vec![a7n_proto::v1::Capability::SettingsSync.into()],
            listen_port: u32::from(endpoint.local_addr()?.port()),
            ..PeerInfo::default()
        };
        let (events, event_rx) = mpsc::channel(MAILBOX_CAPACITY);
        let (inbound, inbound_rx) = mpsc::channel(MAILBOX_CAPACITY);
        let (transfer_tx, transfer_rx) = mpsc::channel(MAILBOX_CAPACITY);
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
            nearby: HashMap::new(),
            discovery_dials: HashSet::new(),
            host: None,
            host_busy: false,
            limiter: AttemptLimiter::new(PAIRING_SOURCES),
            tasks: JoinSet::new(),
            events,
            event_rx,
            inbound,
            inbound_rx,
            transfer_tx,
            transfer_rx,
            input,
            clipboard: clipboard::State::default(),
            config: Config::default(),
            config_path: path.join("config.toml"),
            shutdown_reply: None,
        })
    }

    pub(crate) async fn run(
        mut self,
        commands: mpsc::Receiver<Command>,
        discoveries: mpsc::Receiver<crate::discovery::Event>,
    ) -> Result<(), DaemonError> {
        let result = self.run_loop(commands, discoveries).await;
        self.shutdown_input().await;
        if let Some(reply) = self.shutdown_reply.take() {
            let _ = reply.send(Ok(()));
        }
        result
    }

    async fn run_loop(
        &mut self,
        mut commands: mpsc::Receiver<Command>,
        mut discoveries: mpsc::Receiver<crate::discovery::Event>,
    ) -> Result<(), DaemonError> {
        self.detect_settings_edit()?;
        if self.clipboard.handle.is_none() {
            crate::log::event!("kind=clipboard_unavailable");
        }
        let (accepted, mut accepted_rx) = mpsc::channel(MAILBOX_CAPACITY);
        self.tasks
            .spawn(endpoint::accept_loop(self.endpoint.clone(), accepted));
        let mut retry = tokio::time::interval(nearby::RETRY_INTERVAL);
        retry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut keepalive = tokio::time::interval(a7n_platform::input::KEEPALIVE_INTERVAL);
        keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            let deadline = self.input.focus.next_deadline();
            let clipboard_deadline = self.clipboard.held.as_ref().map(|held| held.deadline);
            tokio::select! {
                command = commands.recv() => {
                    let Some(command) = command else { return Ok(()); };
                    if let Command::Shutdown(reply) = command {
                        self.shutdown_reply = Some(reply);
                        return Ok(());
                    }
                    self.command(command).await?;
                }
                Some((connection, permit, deadline)) = accepted_rx.recv() => {
                    self.accept(connection, permit, deadline)?;
                }
                Some(event) = self.event_rx.recv() => self.event(event),
                Some(event) = discoveries.recv() => self.discovery(event),
                _ = retry.tick() => self.retry_nearby(),
                _ = keepalive.tick() => self.input.keepalive(),
                () = input::deadline(deadline) => self.poll_focus().await,
                () = clipboard::deadline(clipboard_deadline) => self.clipboard.expire_held_clipboard(Instant::now()),
                Some(stream) = self.transfer_rx.recv() => self.clipboard_stream(stream),
                event = clipboard::next(&mut self.clipboard.handle) => match event {
                    clipboard::Event::Changed(change) => self.clipboard_changed(change),
                    clipboard::Event::Paste(request) => self.clipboard_paste(request),
                },
                event = input::captured(&mut self.input.capture) => self.captured(event).await,
                event = input::feedback(&mut self.input.injector) => self.pointer_feedback(event).await,
                Some((peer, id, envelope)) = self.inbound_rx.recv() => {
                    if self.is_current(peer, id) && self.receive_from(peer, envelope).await.is_err() {
                        if let Some(handle) = self.links.by_peer.remove(&peer) {
                            handle.connection.close(0u32.into(), b"");
                        }
                        self.input_link_lost(peer);
                        crate::log::event!("kind=desk_failed");
                    }
                }
                Some(_) = self.tasks.join_next(), if !self.tasks.is_empty() => {}
            }
            self.flush_link_losses().await;
            if self.input.failed {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "input backend closed",
                )
                .into());
            }
        }
    }

    async fn command(&mut self, command: Command) -> Result<(), DaemonError> {
        match command {
            Command::Shutdown(_) => unreachable!("shutdown is handled by the run loop"),
            Command::Switch { machine, reply } => {
                let _ = reply.send(self.switch_machine(machine).await);
            }
            Command::SetPaused { paused, reply } => {
                self.set_paused(paused).await;
                let _ = reply.send(Ok(()));
            }
            Command::ReloadConfig(reply) => {
                let result = self.reload_config().await;
                let _ = reply.send(result);
            }
            Command::Status(reply) => {
                let _ = reply.send(Ok(self.status()));
            }
            Command::ClipboardStatus(reply) => {
                let _ = reply.send(Ok(self.clipboard_status()));
            }
            Command::ClipboardAccept(reply) => {
                let _ = reply.send(self.accept_clipboard());
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
            input: self.input.unavailable.clone(),
            held_clipboard: self.held_clipboard(),
            pid: std::process::id(),
            paused: self.input.paused,
            focused: match self.input.focus.cursor() {
                a7n_core::focus::Cursor::Home => self.identity.machine_id,
                a7n_core::focus::Cursor::Driving(peer) => peer,
            },
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
            nearby: self.nearby_status(),
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
        self.broadcast(&Envelope {
            body: Some(Body::Desk(self.desk.to_desk())),
        });
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
            handle.connection.close(0u32.into(), b"");
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
            handle.connection.close(0u32.into(), b"");
            lost.push(*peer);
            false
        });
        for peer in lost {
            self.input_link_lost(peer);
        }
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
