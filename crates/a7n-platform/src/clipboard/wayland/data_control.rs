// SPDX-License-Identifier: GPL-3.0-or-later

//! Data-control backend: its own Wayland connection, driven from a current-thread
//! tokio runtime so that reads and pastes never block the compositor connection.

mod protocol;

use std::collections::HashMap;
use std::os::fd::{AsFd, OwnedFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use a7n_core::clipboard::LocalChange;
use rustix::pipe::{PipeFlags, pipe_with};
use tokio::io::unix::AsyncFd;
use tokio::sync::mpsc::{self, Receiver, Sender};
use wayland_client::backend::{ObjectId, WaylandError as BackendError};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};
use wayland_protocols::ext::data_control::v1::client::ext_data_control_manager_v1::ExtDataControlManagerV1;
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_manager_v1::ZwlrDataControlManagerV1;

use super::mime::{OWN_MARKER, Read, is_own_selection, plan_reads};
use super::pipe;
use super::served::Served;
use super::transfer::{self, Limiter};
use super::{CHANNEL_CAPACITY, WaylandError};
use crate::clipboard::{Clipboard, ClipboardCommand, PasteRequest};
use protocol::{Device, Manager, Offer, Protocol, Source};

const THREAD_NAME: &str = "a7n-wayland-clipboard";
/// A compositor client could list formats forever.
const MAX_OFFERED_FORMATS: usize = 128;

#[derive(Debug)]
pub(super) enum StartError {
    /// The compositor offers neither data-control protocol.
    Missing,
    Failed(WaylandError),
}

impl<E: Into<WaylandError>> From<E> for StartError {
    fn from(error: E) -> Self {
        Self::Failed(error.into())
    }
}

struct Own {
    source: Source,
    served: Arc<Served>,
}

pub(super) struct State {
    manager: Manager,
    device: Device,
    /// Formats of offers that have not become the selection yet.
    offers: HashMap<ObjectId, Vec<String>>,
    selection: Option<Offer>,
    /// Bumped on every selection change so a slow read never publishes stale data.
    epoch: Arc<AtomicU64>,
    changes: Sender<LocalChange>,
    pastes: Sender<PasteRequest>,
    own: Option<Own>,
    limiter: Limiter,
    finished: bool,
}

pub(super) fn start() -> Result<Clipboard, StartError> {
    let connection = Connection::connect_to_env().map_err(WaylandError::from)?;
    let (globals, queue) = registry_queue_init::<State>(&connection).map_err(WaylandError::from)?;
    let qh = queue.handle();
    let protocol = globals
        .contents()
        .with_list(|list| {
            let names: Vec<_> = list
                .iter()
                .map(|global| global.interface.as_str())
                .collect();
            Protocol::pick(&names)
        })
        .ok_or(StartError::Missing)?;
    let bind_failed = |error: wayland_client::globals::BindError| {
        StartError::Failed(WaylandError::Connection(error.to_string()))
    };
    let seat: WlSeat = globals
        .bind(&qh, 1..=1, ())
        .map_err(|_| StartError::Failed(WaylandError::NoSeat))?;
    let manager = match protocol {
        Protocol::Ext => Manager::Ext(
            globals
                .bind::<ExtDataControlManagerV1, _, _>(&qh, 1..=1, ())
                .map_err(bind_failed)?,
        ),
        Protocol::Wlr => Manager::Wlr(
            globals
                .bind::<ZwlrDataControlManagerV1, _, _>(&qh, 1..=1, ())
                .map_err(bind_failed)?,
        ),
    };
    let device = manager.device(&seat, &qh);

    let (changes_tx, changes) = mpsc::channel(CHANNEL_CAPACITY);
    let (commands, commands_rx) = mpsc::channel(CHANNEL_CAPACITY);
    let (pastes_tx, pastes) = mpsc::channel(CHANNEL_CAPACITY);
    let state = State {
        manager,
        device,
        offers: HashMap::new(),
        selection: None,
        epoch: Arc::new(AtomicU64::new(0)),
        changes: changes_tx,
        pastes: pastes_tx,
        own: None,
        limiter: Limiter::new(),
        finished: false,
    };
    std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || run_thread(&connection, queue, state, commands_rx))
        .map_err(WaylandError::Thread)?;
    Ok(Clipboard {
        images: true,
        changes,
        commands,
        pastes,
    })
}

fn run_thread(
    connection: &Connection,
    queue: EventQueue<State>,
    state: State,
    commands: Receiver<ClipboardCommand>,
) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    let result = match runtime {
        Ok(runtime) => runtime.block_on(run(connection, queue, state, commands)),
        Err(error) => Err(WaylandError::Thread(error)),
    };
    if let Err(error) = result {
        eprintln!("kind=clipboard_wayland_stopped reason={error}");
    }
}

fn connection_error(error: impl std::fmt::Display) -> WaylandError {
    WaylandError::Connection(error.to_string())
}

/// Dispatches events and commands until the daemon or the compositor ends it.
async fn run(
    connection: &Connection,
    mut queue: EventQueue<State>,
    mut state: State,
    mut commands: Receiver<ClipboardCommand>,
) -> Result<(), WaylandError> {
    let qh = queue.handle();
    let socket = connection
        .backend()
        .poll_fd()
        .try_clone_to_owned()
        .map_err(connection_error)?;
    let socket = pipe::async_fd(socket).map_err(connection_error)?;
    loop {
        queue
            .dispatch_pending(&mut state)
            .map_err(connection_error)?;
        if state.finished {
            return Ok(());
        }
        flush(connection, &socket).await?;
        let Some(guard) = queue.prepare_read() else {
            continue;
        };
        tokio::select! {
            ready = socket.readable() => {
                let mut ready = ready.map_err(connection_error)?;
                match guard.read() {
                    Ok(_) => {}
                    Err(BackendError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        // Edge-triggered readiness must stay set until all pending reads are drained.
                        ready.clear_ready();
                    }
                    Err(error) => return Err(connection_error(error)),
                }
            }
            command = commands.recv() => {
                drop(guard);
                let Some(command) = command else {
                    return Ok(());
                };
                state.set(command, &qh);
            }
        }
    }
}

async fn flush(connection: &Connection, socket: &AsyncFd<OwnedFd>) -> Result<(), WaylandError> {
    loop {
        match connection.flush() {
            Ok(()) => return Ok(()),
            Err(BackendError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => {
                socket
                    .writable()
                    .await
                    .map_err(connection_error)?
                    .clear_ready();
            }
            Err(error) => return Err(connection_error(error)),
        }
    }
}

impl State {
    fn on_data_offer(&mut self, offer: &Offer) {
        self.offers.insert(offer.id(), Vec::new());
    }

    fn on_offered_mime(&mut self, offer: &ObjectId, mime: String) {
        if let Some(formats) = self.offers.get_mut(offer) {
            if formats.len() < MAX_OFFERED_FORMATS {
                formats.push(mime);
            }
        }
    }

    fn on_finished(&mut self) {
        self.finished = true;
    }

    /// Primary selection is not synchronised.
    fn on_other_selection(&mut self, offer: Option<Offer>) {
        if let Some(offer) = offer {
            self.offers.remove(&offer.id());
            offer.destroy();
        }
    }

    fn on_selection(&mut self, offer: Option<Offer>) {
        let epoch = self.epoch.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(previous) = self.selection.take() {
            previous.destroy();
        }
        let Some(offer) = offer else {
            return;
        };
        let offered = self.offers.remove(&offer.id()).unwrap_or_default();
        let plan = plan_reads(&offered);
        let reads = if is_own_selection(&offered) {
            None
        } else {
            Some(
                plan.reads
                    .iter()
                    .filter_map(|read| receive(&offer, read))
                    .collect(),
            )
        };
        self.selection = Some(offer);
        if let Some(reads) = reads {
            self.publish(epoch, plan.sensitive, reads);
        }
    }

    fn publish(&self, epoch: u64, sensitive: bool, reads: Vec<(&'static str, OwnedFd)>) {
        let changes = self.changes.clone();
        let current = Arc::clone(&self.epoch);
        tokio::spawn(async move {
            transfer::publish(reads, sensitive, &current, epoch, &changes).await;
        });
    }

    fn on_send(&self, source: &ObjectId, mime: &str, fd: OwnedFd) {
        let Some(own) = self.own.as_ref().filter(|own| own.source.id() == *source) else {
            return;
        };
        // Over the cap the fd is dropped, which ends the paste empty.
        let Some(permit) = self.limiter.acquire() else {
            return;
        };
        let served = Arc::clone(&own.served);
        let pastes = self.pastes.clone();
        let mime = mime.to_owned();
        tokio::spawn(async move {
            let _permit = permit;
            let _ = transfer::answer(&served, &mime, &pastes, fd).await;
        });
    }

    fn on_cancelled(&mut self, source: &Source) {
        if self
            .own
            .as_ref()
            .is_some_and(|own| own.source.id() == source.id())
        {
            self.own = None;
        }
        source.destroy();
    }

    fn set(&mut self, command: ClipboardCommand, qh: &QueueHandle<Self>) {
        let served = Arc::new(Served::new(command));
        let offered = served.offered();
        if offered.is_empty() {
            return;
        }
        // Reads of the selection we replace must not publish any more.
        self.epoch.fetch_add(1, Ordering::SeqCst);
        let source = self.manager.source(qh);
        for mime in offered {
            source.offer(mime);
        }
        source.offer(OWN_MARKER);
        self.device.set_selection(&source);
        if let Some(previous) = self.own.replace(Own { source, served }) {
            previous.source.destroy();
        }
    }
}

/// Asks the selection owner to write one format into a new pipe.
fn receive(offer: &Offer, read: &Read) -> Option<(&'static str, OwnedFd)> {
    let (reader, writer) = pipe_with(PipeFlags::CLOEXEC).ok()?;
    offer.receive(read.offered, writer.as_fd());
    Some((read.item, reader))
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as wayland_client::Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: <WlSeat as wayland_client::Proxy>::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
