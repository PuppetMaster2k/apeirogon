// SPDX-License-Identifier: GPL-3.0-or-later

//! Clipboard portal backend. The `RemoteDesktop` session belongs to the input
//! backend, so this service runs inside its runtime and reaches the daemon
//! through a [`Clipboard`] left in a process-wide slot.
//!
//! Order: `input::start()` fills the slot before it returns, `clipboard::start()`
//! empties it. Started the other way round, the clipboard finds nothing.

use std::pin::pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use a7n_core::clipboard::LocalChange;
use ashpd::desktop::Session;
use ashpd::desktop::clipboard::{
    Clipboard as PortalClipboard, RequestClipboardOptions, SelectionOwnerChanged,
    SetSelectionOptions,
};
use ashpd::desktop::remote_desktop::RemoteDesktop;
use futures_util::StreamExt;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::oneshot;

use super::CHANNEL_CAPACITY;
use super::mime::plan_reads;
use super::served::Served;
use super::transfer::{self, Limiter};
use crate::clipboard::{Clipboard, ClipboardCommand, PasteRequest};

static HANDOFF: Mutex<Option<Clipboard>> = Mutex::new(None);

/// The clipboard the input backend obtained, once.
pub(super) fn take() -> Option<Clipboard> {
    HANDOFF
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
}

/// A portal signal, forwarded from the task that owns the subscriptions (the
/// streams borrow the portal proxy).
enum Signal {
    OwnerChanged(Session<RemoteDesktop>, SelectionOwnerChanged),
    Transfer(Session<RemoteDesktop>, String, u32),
}

async fn forward(
    portal: Arc<PortalClipboard>,
    signals: Sender<Signal>,
    ready: oneshot::Sender<Result<(), ashpd::Error>>,
) {
    let subscribed = async {
        let owner_changes = portal
            .receive_selection_owner_changed::<RemoteDesktop>()
            .await?;
        let transfers = portal.receive_selection_transfer::<RemoteDesktop>().await?;
        Ok::<_, ashpd::Error>((owner_changes, transfers))
    }
    .await;
    let (owner_changes, transfers) = match subscribed {
        Ok(streams) => streams,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    if ready.send(Ok(())).is_err() {
        return;
    }
    let mut owner_changes = pin!(owner_changes);
    let mut transfers = pin!(transfers);
    loop {
        let signal = tokio::select! {
            Some((session, changed)) = owner_changes.next() => Signal::OwnerChanged(session, changed),
            Some((session, mime, serial)) = transfers.next() => Signal::Transfer(session, mime, serial),
            else => return,
        };
        if signals.send(signal).await.is_err() {
            return;
        }
    }
}

/// Asks for clipboard access on a session that is not started yet. `None`
/// when the portal lacks the interface; input keeps working without it.
pub(crate) async fn request(session: &Session<RemoteDesktop>) -> Option<PortalClipboard> {
    let portal = PortalClipboard::new().await;
    let requested = match portal {
        Ok(portal) => portal
            .request(session, RequestClipboardOptions::default())
            .await
            .map(|()| portal),
        Err(error) => Err(error),
    };
    match requested {
        Ok(portal) => Some(portal),
        Err(error) => {
            eprintln!("kind=clipboard_wayland_portal_unavailable reason={error}");
            None
        }
    }
}

pub(crate) struct Service {
    portal: Arc<PortalClipboard>,
    session: Arc<Session<RemoteDesktop>>,
    signals: Receiver<Signal>,
    changes: Sender<LocalChange>,
    commands: Receiver<ClipboardCommand>,
    pastes: Sender<PasteRequest>,
    handle: Option<Clipboard>,
}

impl Service {
    /// Subscribes to the portal signals; call right after the session started
    /// with clipboard access so no early selection is missed.
    pub(crate) async fn connect(
        portal: PortalClipboard,
        session: Arc<Session<RemoteDesktop>>,
    ) -> Result<Self, ashpd::Error> {
        let portal = Arc::new(portal);
        let (signals_tx, signals) = mpsc::channel(CHANNEL_CAPACITY);
        let (ready_tx, ready) = oneshot::channel();
        tokio::spawn(forward(Arc::clone(&portal), signals_tx, ready_tx));
        ready.await.map_err(|_| ashpd::Error::NoResponse)??;
        let (changes_tx, changes) = mpsc::channel(CHANNEL_CAPACITY);
        let (commands, commands_rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (pastes_tx, pastes) = mpsc::channel(CHANNEL_CAPACITY);
        Ok(Self {
            portal,
            session,
            signals,
            changes: changes_tx,
            commands: commands_rx,
            pastes: pastes_tx,
            handle: Some(Clipboard {
                images: true,
                changes,
                commands,
                pastes,
            }),
        })
    }

    /// Makes the clipboard available to `clipboard::start()`. Called once the
    /// input backend is fully up.
    pub(crate) fn hand_over(&mut self) {
        *HANDOFF.lock().unwrap_or_else(PoisonError::into_inner) = self.handle.take();
    }

    /// Runs until the daemon drops its clipboard or the portal ends the signals.
    pub(crate) async fn run(mut self) {
        let epoch = Arc::new(AtomicU64::new(0));
        let limiter = Limiter::new();
        let mut served: Option<Arc<Served>> = None;
        loop {
            tokio::select! {
                signal = self.signals.recv() => match signal {
                    Some(Signal::OwnerChanged(session, changed)) => {
                        self.on_owner_changed(session, &changed, &epoch);
                    }
                    Some(Signal::Transfer(session, mime, serial)) => {
                        self.on_transfer(session, mime, serial, served.as_ref(), &limiter);
                    }
                    None => break,
                },
                command = self.commands.recv() => {
                    let Some(command) = command else {
                        break;
                    };
                    self.set(command, &mut served, &epoch).await;
                }
            }
        }
    }

    fn on_owner_changed(
        &self,
        session: Session<RemoteDesktop>,
        changed: &SelectionOwnerChanged,
        epoch: &Arc<AtomicU64>,
    ) {
        let stamp = epoch.fetch_add(1, Ordering::SeqCst) + 1;
        // Our own `SetSelection` coming back.
        if changed.session_is_owner() == Some(true) {
            return;
        }
        let plan = plan_reads(changed.mime_types());
        let portal = Arc::clone(&self.portal);
        let sink = self.changes.clone();
        let epoch = Arc::clone(epoch);
        tokio::spawn(async move {
            let mut reads = Vec::new();
            for read in plan.reads {
                if let Ok(fd) = portal.selection_read(&session, read.offered).await {
                    reads.push((read.item, fd.into()));
                }
            }
            transfer::publish(reads, plan.sensitive, &epoch, stamp, &sink).await;
        });
    }

    fn on_transfer(
        &self,
        session: Session<RemoteDesktop>,
        mime: String,
        serial: u32,
        served: Option<&Arc<Served>>,
        limiter: &Limiter,
    ) {
        let served = served.cloned();
        let Some(permit) = limiter.acquire() else {
            return;
        };
        let portal = Arc::clone(&self.portal);
        let pastes = self.pastes.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let Ok(fd) = portal.selection_write(&session, serial).await else {
                return;
            };
            let success = match served {
                Some(served) => transfer::answer(&served, &mime, &pastes, fd.into()).await,
                None => false,
            };
            let _ = portal.selection_write_done(&session, serial, success).await;
        });
    }

    async fn set(
        &self,
        command: ClipboardCommand,
        served: &mut Option<Arc<Served>>,
        epoch: &AtomicU64,
    ) {
        let next = Arc::new(Served::new(command));
        let offered = next.offered();
        if offered.is_empty() {
            return;
        }
        // Reads of the selection we replace must not publish any more.
        epoch.fetch_add(1, Ordering::SeqCst);
        *served = Some(next);
        let options = SetSelectionOptions::default().set_mime_types(&offered);
        if let Err(error) = self.portal.set_selection(&*self.session, options).await {
            *served = None;
            eprintln!("kind=clipboard_wayland_portal_set_failed reason={error}");
        }
    }
}
