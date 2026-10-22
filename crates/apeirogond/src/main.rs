// SPDX-License-Identifier: GPL-3.0-or-later
#![forbid(unsafe_code)]

mod command;
mod daemon;
mod desk_store;
mod endpoint;
mod handshake;
mod identity;
mod ipc_server;
mod link;
mod link_driver;
mod pairing;
mod pin_store;
mod tls;

use std::net::{Ipv4Addr, SocketAddr};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<()> {
    let dirs = ProjectDirs::from("", "", "apeirogon").context("no home directory found")?;
    let daemon = daemon::Daemon::load(
        dirs.data_local_dir(),
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, a7n_proto::limits::DEFAULT_UDP_PORT)),
    )?;
    let (commands, receiver) = mpsc::channel(1);
    let server = a7n_ipc::IpcServer::bind()
        .await
        .context("cannot open the control socket; is apeirogond already running?")?;
    tokio::spawn(ipc_server::serve(server, commands));
    tokio::select! {
        result = daemon.run(receiver) => result?,
        result = tokio::signal::ctrl_c() => result.context("cannot listen for ctrl-c")?,
    }
    Ok(())
}
