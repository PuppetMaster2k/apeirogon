// SPDX-License-Identifier: GPL-3.0-or-later
#![forbid(unsafe_code)]

mod command;
mod daemon;
mod desk_store;
mod discovery;
mod endpoint;
mod handshake;
mod identity;
mod input_retry;
mod ipc_server;
mod layout_store;
mod link;
mod link_driver;
mod log;
mod pairing;
mod pin_store;
mod settings_store;
mod tls;

use std::net::{Ipv4Addr, SocketAddr};

use a7n_ipc::config::Config;
use anyhow::{Context, Result};
use clap::Parser;
use directories::ProjectDirs;
use tokio::sync::{mpsc, oneshot};

#[derive(Parser)]
#[command(version, about = "Apeirogon session daemon")]
struct Cli {
    /// Log physical key codes for input debugging.
    #[arg(long)]
    trace_input: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.trace_input {
        log::event!("kind=trace_input_warning key codes will be logged; do not type secrets");
    }
    let dirs = ProjectDirs::from("", "", "apeirogon").context("no home directory found")?;
    let config_path = Config::path()?;
    let config = Config::load(&config_path)?;
    let server = a7n_ipc::IpcServer::bind()
        .await
        .context("cannot open the control socket; is apeirogond already running?")?;
    let (input, input_error, retry) = match a7n_platform::input::start() {
        Ok(handles) => (Some(handles), None, None),
        Err(error) => {
            crate::log::event!("kind=input_unavailable reason={error}");
            let retry = (!a7n_platform::permissions::missing().is_empty()).then(|| {
                let (handles, receiver) = oneshot::channel();
                input_retry::spawn(handles);
                receiver
            });
            (None, Some(error.to_string()), retry)
        }
    };
    let mut daemon = daemon::Daemon::load(
        dirs.data_local_dir(),
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, config.port)),
    )?
    .with_config(config, config_path)
    .with_input(input)
    .with_input_error(input_error)
    .with_clipboard(a7n_platform::clipboard::start().ok())
    .with_trace_input(cli.trace_input);
    if let Some(retry) = retry {
        daemon = daemon.with_input_retry(retry);
    }
    let (discoveries, discovery_rx) = mpsc::channel(discovery::CHANNEL_CAPACITY);
    let (machine, port) = daemon.discovery_identity()?;
    let discovery_task = tokio::spawn(discovery::run(machine, port, discoveries));
    let (commands, receiver) = mpsc::channel(1);
    let ipc_task = tokio::spawn(ipc_server::serve(server, commands));
    tokio::select! {
        result = daemon.run(receiver, discovery_rx) => {
            result?;
            ipc_task.await.context("control server failed")?;
        },
        result = tokio::signal::ctrl_c() => result.context("cannot listen for ctrl-c")?,
    }
    discovery_task.abort();
    Ok(())
}
