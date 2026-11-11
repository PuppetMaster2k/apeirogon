// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::io;
use std::net::{Ipv4Addr, UdpSocket};
use std::time::{Duration, Instant};

use a7n_proto::{MachineId, v1::Landing};

use super::{Daemon, DaemonError};

pub(super) const WAKE_RESEND_INTERVAL: Duration = Duration::from_secs(5);
pub(super) const WAKE_TIMEOUT: Duration = Duration::from_secs(60);
const WAKE_PORT: u16 = 9;

type PacketSender = Box<dyn FnMut([u8; 6]) -> io::Result<()> + Send>;

pub(super) struct Pending {
    pub(super) peer: MachineId,
    pub(super) deadline: Instant,
    action: WakeAction,
}

pub(super) enum WakeAction {
    Only,
    Switch,
    Edge(Landing),
}

pub(super) struct State {
    pub(super) pending: Option<Pending>,
    sent: HashMap<MachineId, Instant>,
    pub(super) sender: PacketSender,
}

impl Default for State {
    fn default() -> Self {
        Self {
            pending: None,
            sent: HashMap::new(),
            sender: Box::new(send_packet),
        }
    }
}

fn send_packet(mac: [u8; 6]) -> io::Result<()> {
    let packet = a7n_core::wol::magic_packet(mac);
    let mut targets = a7n_platform::ipv4_broadcasts();
    targets.push((Ipv4Addr::UNSPECIFIED, Ipv4Addr::BROADCAST));
    targets.sort_unstable();
    targets.dedup();
    let mut sent = false;
    let mut error = None;
    for (source, broadcast) in targets {
        let result = (|| {
            let socket = UdpSocket::bind((source, 0))?;
            socket.set_broadcast(true)?;
            socket.set_nonblocking(true)?;
            socket.send_to(&packet, (broadcast, WAKE_PORT))
        })();
        match result {
            Ok(_) => sent = true,
            Err(failure) => error = Some(failure),
        }
    }
    if sent {
        Ok(())
    } else {
        Err(error.unwrap_or_else(|| io::Error::other("no broadcast interface")))
    }
}

impl Daemon {
    pub(super) fn wake_peer(
        &mut self,
        peer: MachineId,
        action: WakeAction,
        now: Instant,
    ) -> Result<(), DaemonError> {
        let entry = self.pins.get(&peer).ok_or(DaemonError::NotPaired)?;
        if self.is_linked(peer) {
            return Err(DaemonError::InvalidRequest("machine is already online"));
        }
        let mac = entry.hw_addr.ok_or(DaemonError::InvalidRequest(
            "no hardware address known; connect once while awake first",
        ))?;
        if self
            .wol
            .sent
            .get(&peer)
            .is_none_or(|last| now.saturating_duration_since(*last) >= WAKE_RESEND_INTERVAL)
        {
            (self.wol.sender)(mac)?;
            self.wol.sent.retain(|id, _| self.pins.get(id).is_some());
            self.wol.sent.insert(peer, now);
        }
        self.wol.pending = Some(Pending {
            peer,
            deadline: now + WAKE_TIMEOUT,
            action,
        });
        Ok(())
    }

    pub(super) fn auto_wake(
        &mut self,
        peer: MachineId,
        action: WakeAction,
        now: Instant,
    ) -> Result<(), DaemonError> {
        let same = self
            .wol
            .pending
            .as_ref()
            .is_some_and(|pending| pending.peer == peer && pending.deadline > now);
        if !same {
            self.wol.pending = None;
        }
        if self
            .pins
            .get(&peer)
            .is_none_or(|entry| entry.hw_addr.is_none())
            || self.config.peer(&peer).is_some_and(|config| !config.wake)
        {
            return Err(DaemonError::NotConnected);
        }
        if same {
            if let Some(pending) = &mut self.wol.pending
                && matches!(pending.action, WakeAction::Only)
            {
                pending.action = action;
            }
            return Ok(());
        }
        self.wake_peer(peer, action, now)
    }

    pub(super) async fn finish_wake(&mut self, now: Instant) {
        let Some(pending) = &self.wol.pending else {
            return;
        };
        if pending.deadline <= now || self.pins.get(&pending.peer).is_none() {
            self.wol.pending = None;
            return;
        }
        if !self.is_linked(pending.peer) {
            return;
        }
        let pending = self.wol.pending.take().expect("pending wake checked above");
        let landing = match pending.action {
            WakeAction::Only => return,
            WakeAction::Switch => {
                if self.switch_machine(pending.peer).await.is_err() {
                    crate::log::event!("kind=wake_switch_failed");
                }
                return;
            }
            WakeAction::Edge(landing) => landing,
        };
        if self.input.paused || !self.is_layout_target(pending.peer) {
            return;
        }
        let actions = self.take_peer(pending.peer, Some(landing), now);
        self.input_actions(actions).await;
    }
}

#[cfg(test)]
mod tests;
