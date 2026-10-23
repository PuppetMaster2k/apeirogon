// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::config::Config;
use a7n_ipc::{IpcClient, IpcError, Request, Response};
use anyhow::{Result, bail};
use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Print the config file path.
    Path,
    /// Print all settings, including defaults.
    Show,
    /// Print one dotted setting key.
    Get { key: String },
    /// Validate, save and reload a setting. Rewrites the file without comments.
    Set { key: String, value: String },
}

pub(crate) fn init() -> Result<()> {
    let path = Config::path()?;
    Config::init(&path)?;
    println!("{}", path.display());
    Ok(())
}

pub(crate) async fn run(command: Command) -> Result<()> {
    let path = Config::path()?;
    if matches!(command, Command::Path) {
        println!("{}", path.display());
        return Ok(());
    }
    let mut config = Config::load(&path)?;
    match command {
        Command::Path => unreachable!("path was handled before loading config"),
        Command::Show => print!("{}", config.to_toml()?),
        Command::Get { key } => {
            let Some(value) = config.get(&key) else {
                bail!("{key} is not set or is unknown");
            };
            println!("{value}");
        }
        Command::Set { key, value } => {
            config.set(&key, &value)?;
            config.save(&path)?;
            if matches!(key.as_str(), "name" | "port") {
                eprintln!("takes effect after apeirogond restarts");
            }
            reload().await?;
        }
    }
    Ok(())
}

async fn reload() -> Result<()> {
    let mut client = match IpcClient::connect().await {
        Ok(client) => client,
        Err(IpcError::Socket(_)) => {
            eprintln!("apeirogond is not running; applies at next start");
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    match client.request(&Request::ReloadConfig).await? {
        Response::Done => Ok(()),
        Response::Error { message, .. } => bail!("{message}"),
        _ => bail!("expected reload completion from daemon"),
    }
}
