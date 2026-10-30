// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::{ClipboardKind, HeldClipboard};

use super::{CLIPBOARD_ASK_TIMEOUT, Daemon, Held, Instant, MachineId, State, clipboard};
use crate::daemon::DaemonError;

pub(in crate::daemon) async fn deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
        None => std::future::pending().await,
    }
}

impl Held {
    fn expired(&self, now: Instant) -> bool {
        now >= self.deadline
    }
}

impl State {
    pub(in crate::daemon) fn drop_held(&mut self) {
        if self.held.take().is_some() {
            crate::log::event!("kind=clipboard_dropped");
        }
    }

    pub(in crate::daemon) fn expire_held_clipboard(&mut self, now: Instant) {
        if self.held.as_ref().is_some_and(|held| held.expired(now)) {
            self.held = None;
            crate::log::event!("kind=clipboard_ask_expired");
        }
    }
}

impl Daemon {
    pub(in crate::daemon) fn held_clipboard(&self) -> Option<HeldClipboard> {
        let held = self.clipboard.held.as_ref()?;
        let mut kinds = Vec::new();
        for item in &held.wire.items {
            let kind = match item.mime.as_str() {
                clipboard::TEXT_PLAIN | clipboard::TEXT_HTML => ClipboardKind::Text,
                clipboard::IMAGE_PNG => ClipboardKind::Image,
                _ => continue,
            };
            if !kinds.contains(&kind) {
                kinds.push(kind);
            }
        }
        Some(HeldClipboard {
            machine: held.peer,
            name: self
                .pins
                .get(&held.peer)
                .map_or_else(|| held.peer.to_string(), |entry| entry.name.clone()),
            items: u32::try_from(held.wire.items.len()).ok()?,
            size: held.wire.items.iter().map(|item| item.size).sum(),
            kinds,
        })
    }

    pub(in crate::daemon) fn hold_clipboard(
        &mut self,
        peer: MachineId,
        wire: &a7n_proto::v1::ClipboardAnnounce,
    ) {
        match clipboard::accept(self.clipboard.current, wire) {
            Ok(Some(_)) => {}
            Ok(None) => return,
            Err(_) => {
                crate::log::event!("kind=clipboard_rejected");
                return;
            }
        }
        if self.clipboard.held.as_ref().is_some_and(|held| {
            (wire.generation, wire.origin.as_slice())
                <= (held.wire.generation, held.wire.origin.as_slice())
        }) {
            return;
        }
        self.clipboard.drop_held();
        self.clipboard.held = Some(Held {
            peer,
            wire: wire.clone(),
            deadline: Instant::now() + CLIPBOARD_ASK_TIMEOUT,
        });
        crate::log::event!("kind=clipboard_held");
    }

    pub(in crate::daemon) fn accept_clipboard(&mut self) -> Result<(), DaemonError> {
        self.clipboard.expire_held_clipboard(Instant::now());
        let held = self
            .clipboard
            .held
            .take()
            .ok_or(DaemonError::InvalidRequest(
                "no clipboard copy is waiting for confirmation",
            ))?;
        if !self.links.by_peer.get(&held.peer).is_some_and(|link| {
            link.connection.close_reason().is_none() && !link.sender.is_closed()
        }) {
            crate::log::event!("kind=clipboard_dropped");
            return Err(DaemonError::NotConnected);
        }
        if !matches!(
            clipboard::accept(self.clipboard.current, &held.wire),
            Ok(Some(_))
        ) {
            crate::log::event!("kind=clipboard_dropped");
            return Err(DaemonError::InvalidRequest(
                "clipboard copy is no longer current",
            ));
        }
        self.apply_clipboard(&held.wire);
        crate::log::event!("kind=clipboard_accepted");
        Ok(())
    }

    pub(in crate::daemon) fn reject_clipboard(&mut self) -> Result<(), DaemonError> {
        if self.clipboard.held.is_none() {
            return Err(DaemonError::InvalidRequest(
                "no clipboard copy is waiting for confirmation",
            ));
        }
        self.clipboard.drop_held();
        Ok(())
    }

    pub(in crate::daemon) fn clipboard_link_down(&mut self, peer: MachineId) {
        self.clipboard.links.remove(&peer);
        if self
            .clipboard
            .held
            .as_ref()
            .is_some_and(|held| held.peer == peer)
        {
            self.clipboard.drop_held();
        }
    }
}
