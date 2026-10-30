// SPDX-License-Identifier: GPL-3.0-or-later

//! The in-process command seam consumed by the #23 IPC server.

use std::net::SocketAddr;

use a7n_core::pairing::PairingCode;
pub(crate) use a7n_ipc::StatusInfo as Status;
use a7n_proto::MachineId;
use tokio::sync::oneshot;

use crate::daemon::DaemonError;
use crate::pin_store::Entry;

pub(crate) type Reply<T> = oneshot::Sender<Result<T, DaemonError>>;

#[derive(Debug)]
pub(crate) enum Command {
    ReloadConfig(Reply<()>),
    Shutdown(Reply<()>),
    SetPaused {
        paused: bool,
        reply: Reply<()>,
    },
    Switch {
        machine: MachineId,
        reply: Reply<()>,
    },
    Status(Reply<Status>),
    ClipboardStatus(Reply<a7n_ipc::ClipboardInfo>),
    ClipboardAccept(Reply<()>),
    ClipboardReject(Reply<()>),
    GetLayout(Reply<a7n_ipc::LayoutInfo>),
    SetLayout {
        placed: Vec<a7n_ipc::LayoutPlacement>,
        reply: Reply<()>,
    },
    PairHost(Reply<PairingCode>),
    PairJoin {
        addr: SocketAddr,
        code: PairingCode,
        reply: Reply<Entry>,
    },
    Unpair {
        machine: MachineId,
        reply: Reply<()>,
    },
    Connect {
        addr: SocketAddr,
        reply: Reply<()>,
    },
    RequestPermission {
        permission: a7n_platform::permissions::Permission,
        reply: Reply<()>,
    },
}
