// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::{Path, PathBuf};

use a7n_ipc::Request;
use a7n_platform::autostart;
use anyhow::{Result, bail};
use clap::Subcommand;

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
        Command::Start => println!("running (pid {})", a7n_ipc::daemon::start().await?),
        Command::Stop => {
            let mut client = super::connect_client().await?;
            super::write_response(
                &mut std::io::stdout().lock(),
                &client.request(&Request::Shutdown).await?,
            )?;
        }
        Command::Status => {
            if let Some(pid) = a7n_ipc::daemon::status().await {
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
