// SPDX-License-Identifier: GPL-3.0-or-later
#![forbid(unsafe_code)]

mod command;
mod daemon;
mod desk_store;
mod discovery;
mod endpoint;
mod handshake;
mod identity;
mod ipc_server;
mod layout_store;
mod link;
mod link_driver;
mod pairing;
mod pin_store;
mod settings_store;
mod tls;

use std::net::{Ipv4Addr, SocketAddr};

use a7n_ipc::config::Config;
use anyhow::{Context, Result};
use directories::ProjectDirs;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<()> {
    let dirs = ProjectDirs::from("", "", "apeirogon").context("no home directory found")?;
    let config_path = Config::path()?;
    let config = Config::load(&config_path)?;
    let input = a7n_platform::input::start().map_or_else(
        |_| {
            eprintln!("kind=input_unavailable");
            None
        },
        Some,
    );
    let daemon = daemon::Daemon::load(
        dirs.data_local_dir(),
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, config.port)),
    )?
    .with_config(config, config_path)
    .with_input(input);
    let (discoveries, discovery_rx) = mpsc::channel(discovery::CHANNEL_CAPACITY);
    let (machine, port) = daemon.discovery_identity()?;
    let discovery_task = tokio::spawn(discovery::run(machine, port, discoveries));
    let (commands, receiver) = mpsc::channel(1);
    let server = a7n_ipc::IpcServer::bind()
        .await
        .context("cannot open the control socket; is apeirogond already running?")?;
    tokio::spawn(ipc_server::serve(server, commands));
    tokio::select! {
        result = daemon.run(receiver, discovery_rx) => result?,
        result = tokio::signal::ctrl_c() => result.context("cannot listen for ctrl-c")?,
    }
    discovery_task.abort();
    Ok(())
}
