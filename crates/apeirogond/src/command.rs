// SPDX-License-Identifier: GPL-3.0-or-later

//! The in-process command seam consumed by the #23 IPC server.

use std::net::SocketAddr;

use a7n_core::pairing::PairingCode;
use a7n_proto::MachineId;
use a7n_proto::v1::{Capability, PeerInfo};
use tokio::sync::oneshot;

use crate::daemon::DaemonError;

pub(crate) type Reply<T> = oneshot::Sender<Result<T, DaemonError>>;

#[derive(Debug)]
#[allow(dead_code, reason = "#23 IPC consumes these commands in the next PR")]
pub(crate) enum Command {
    Status(Reply<Status>),
    PairHost(Reply<PairingCode>),
    PairJoin {
        addr: SocketAddr,
        code: PairingCode,
        reply: Reply<()>,
    },
    Unpair {
        machine: MachineId,
        reply: Reply<()>,
    },
    Connect {
        addr: SocketAddr,
        reply: Reply<()>,
    },
}

#[derive(Debug)]
#[allow(dead_code, reason = "#23 IPC serializes status in the next PR")]
pub(crate) struct Status {
    pub(crate) machine: MachineId,
    pub(crate) addr: SocketAddr,
    pub(crate) paired: Vec<MachineId>,
    pub(crate) linked: Vec<LinkStatus>,
}

#[derive(Debug)]
#[allow(dead_code, reason = "#23 IPC serializes link status in the next PR")]
pub(crate) struct LinkStatus {
    pub(crate) machine: MachineId,
    pub(crate) info: PeerInfo,
    pub(crate) capabilities: Vec<Capability>,
    pub(crate) role: crate::link::Role,
}
