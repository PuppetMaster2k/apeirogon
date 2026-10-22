// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Duration;

use a7n_proto::MachineId;
use tokio::sync::oneshot;

use super::{Daemon, Event};
use crate::discovery::{Event as DiscoveryEvent, MAX_NEARBY};
use crate::{endpoint, link::LinkError};

pub(super) const RETRY_INTERVAL: Duration = Duration::from_secs(10);

#[cfg(test)]
mod tests;

impl Daemon {
    pub(crate) fn discovery_identity(&self) -> Result<(MachineId, u16), std::io::Error> {
        Ok((self.identity.machine_id, self.endpoint.local_addr()?.port()))
    }

    pub(super) fn discovery(&mut self, event: DiscoveryEvent) {
        match event {
            DiscoveryEvent::Discovered(announcement) => {
                if announcement.id == self.identity.machine_id || announcement.addrs.is_empty() {
                    return;
                }
                if self.nearby.len() >= MAX_NEARBY && !self.nearby.contains_key(&announcement.id) {
                    return;
                }
                self.nearby.insert(announcement.id, announcement.addrs);
                self.dial_nearby(announcement.id);
            }
            DiscoveryEvent::Lost(id) => {
                self.nearby.remove(&id);
            }
        }
    }

    pub(super) fn nearby_status(&self) -> Vec<a7n_ipc::NearbyMachine> {
        let mut nearby: Vec<_> = self
            .nearby
            .iter()
            .filter(|(id, _)| self.pins.get(id).is_none())
            .filter_map(|(&id, addrs)| {
                addrs
                    .first()
                    .map(|&addr| a7n_ipc::NearbyMachine { id, addr })
            })
            .collect();
        nearby.sort_unstable_by_key(|peer| peer.id);
        nearby
    }

    pub(super) fn retry_nearby(&mut self) {
        for id in self.nearby.keys().copied().collect::<Vec<_>>() {
            self.dial_nearby(id);
        }
    }

    fn dial_nearby(&mut self, id: MachineId) {
        if self.pins.get(&id).is_none()
            || self.discovery_dials.contains(&id)
            || self.discovery_dials.len() >= MAX_NEARBY
            || self.links.by_peer.get(&id).is_some_and(|handle| {
                handle.connection.close_reason().is_none() && !handle.sender.is_closed()
            })
        {
            return;
        }
        let Some(addrs) = self.nearby.get(&id).cloned() else {
            return;
        };
        self.discovery_dials.insert(id);
        let endpoint = self.endpoint.clone();
        let info = self.info.clone();
        let events = self.events.clone();
        self.tasks.spawn(async move {
            for addr in addrs {
                let result = endpoint::dial(&endpoint, addr, &info).await;
                let result = match result {
                    Ok(link) if link.peer_id != id => {
                        // A LAN hint must not redirect us to another paired machine.
                        link.connection.close(0u32.into(), b"");
                        continue;
                    }
                    Ok(link) => Ok(Box::new(link)),
                    Err(LinkError::Superseded(peer)) if peer == id => {
                        Err(LinkError::Superseded(peer))
                    }
                    Err(_) => continue,
                };
                let (reply, receiver) = oneshot::channel();
                if events.send(Event::Link(result, Some(reply))).await.is_err() {
                    return;
                }
                if matches!(receiver.await, Ok(Ok(()))) {
                    break;
                }
            }
            let _ = events.send(Event::DiscoveryDialEnded(id)).await;
        });
    }
}
