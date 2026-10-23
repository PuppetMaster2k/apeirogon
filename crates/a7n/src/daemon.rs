// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::{Path, PathBuf};
use std::time::Duration;

use a7n_ipc::{IpcClient, Request, Response};
use a7n_platform::autostart;
use anyhow::{Context, Result, bail};
use clap::Subcommand;

const START_TIMEOUT: Duration = Duration::from_secs(3);
const START_POLL: Duration = Duration::from_millis(50);

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    Start,
    Stop,
    Status,
    Enable,
    Disable,
}

pub(crate) async fn run(command: Command) -> Result<()> {
    match command {
        Command::Start => start().await?,
        Command::Stop => {
            let mut client = super::connect_client().await?;
            super::write_response(
                &mut std::io::stdout().lock(),
                &client.request(&Request::Shutdown).await?,
            )?;
        }
        Command::Status => {
            if let Some(pid) = status().await {
                println!("running (pid {pid})");
            } else {
                println!("not running");
            }
            println!(
                "start at login: {}",
                if autostart::is_enabled()? {
                    "enabled"
                } else {
                    "disabled"
                }
            );
        }
        Command::Enable => {
            let exe = absolute_executable(&autostart::daemon_executable()?)?;
            if let Some(notice) = autostart::enable(&exe)? {
                eprintln!("{notice}");
            }
        }
        Command::Disable => {
            if let Some(notice) = autostart::disable()? {
                eprintln!("{notice}");
            }
        }
    }
    Ok(())
}

async fn status() -> Option<u32> {
    tokio::time::timeout(START_TIMEOUT, async {
        let mut client = IpcClient::connect().await.ok()?;
        match client.request(&Request::Status).await.ok()? {
            Response::Status(status) => Some(status.pid),
            _ => None,
        }
    })
    .await
    .ok()
    .flatten()
}

async fn start() -> Result<()> {
    if let Some(pid) = status().await {
        println!("running (pid {pid})");
        return Ok(());
    }
    autostart::start(&autostart::daemon_executable()?).context("cannot start apeirogond")?;
    tokio::time::timeout(START_TIMEOUT, async {
        loop {
            if let Some(pid) = status().await {
                println!("running (pid {pid})");
                return;
            }
            tokio::time::sleep(START_POLL).await;
        }
    })
    .await
    .context("apeirogond did not start within 3 s; check a7n logs")?;
    Ok(())
}

fn absolute_executable(exe: &Path) -> Result<PathBuf> {
    if exe.is_absolute() {
        return Ok(exe.to_owned());
    }
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        let candidate = dir.join(exe);
        if candidate.is_file() {
            return Ok(candidate.canonicalize()?);
        }
    }
    bail!("cannot find apeirogond in PATH");
}
