// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::{HashMap, hash_map::Entry};
use std::time::{Duration, Instant};

use a7n_proto::MachineId;
use a7n_proto::v1::{Capability, Envelope, envelope::Body};

use super::{Daemon, DaemonError};

pub(super) const SYNC_MIN_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Layout,
    Settings,
    Desk,
}

struct Pending {
    last: Instant,
    message: Option<Envelope>,
}

#[derive(Default)]
pub(super) struct State {
    incoming: HashMap<(MachineId, Kind), Pending>,
}

impl State {
    pub(super) fn drop_peer(&mut self, peer: MachineId) {
        self.incoming.retain(|(id, _), _| *id != peer);
    }

    fn coalesce(&mut self, peer: MachineId, message: Envelope, now: Instant) -> Option<Envelope> {
        let kind = match message.body {
            Some(Body::Layout(_)) => Kind::Layout,
            Some(Body::Settings(_)) => Kind::Settings,
            Some(Body::Desk(_)) => Kind::Desk,
            _ => return Some(message),
        };
        let pending = match self.incoming.entry((peer, kind)) {
            Entry::Vacant(entry) => {
                entry.insert(Pending {
                    last: now,
                    message: None,
                });
                return Some(message);
            }
            Entry::Occupied(entry) => entry.into_mut(),
        };
        if now.duration_since(pending.last) < SYNC_MIN_INTERVAL {
            pending.message = Some(message);
            return None;
        }
        pending.last = now;
        pending.message = None;
        Some(message)
    }
}

impl Daemon {
    pub(super) async fn receive_sync_from(
        &mut self,
        peer: MachineId,
        message: Envelope,
        now: Instant,
    ) -> Result<(), DaemonError> {
        if let Some(message) = self.sync.coalesce(peer, message, now) {
            self.receive_sync(peer, message).await?;
        }
        Ok(())
    }

    async fn receive_sync(
        &mut self,
        peer: MachineId,
        message: Envelope,
    ) -> Result<(), DaemonError> {
        if let Some(Body::Settings(settings)) = &message.body {
            if self
                .links
                .by_peer
                .get(&peer)
                .is_some_and(|handle| handle.capabilities.contains(&Capability::SettingsSync))
            {
                self.receive_settings(settings).await?;
            }
            return Ok(());
        }
        self.receive(message)
    }

    pub(super) async fn flush_sync(&mut self, now: Instant) {
        let keys: Vec<_> = self.sync.incoming.keys().copied().collect();
        for key in keys {
            let Some(pending) = self.sync.incoming.get_mut(&key) else {
                continue;
            };
            if now.duration_since(pending.last) < SYNC_MIN_INTERVAL {
                continue;
            }
            let Some(message) = pending.message.take() else {
                continue;
            };
            pending.last = now;
            if self.receive_sync(key.0, message).await.is_err() {
                self.peer_message_failed(key.0);
            }
        }
    }
}

#[cfg(test)]
mod tests;
