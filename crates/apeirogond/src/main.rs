// SPDX-License-Identifier: GPL-3.0-or-later
#![forbid(unsafe_code)]

#[allow(
    dead_code,
    reason = "consumed by pairing (#7) and the daemon loop (#23)"
)]
mod endpoint;
#[allow(
    dead_code,
    reason = "consumed by pairing (#7) and the daemon loop (#23)"
)]
mod handshake;
#[allow(
    dead_code,
    reason = "consumed by pairing (#7) and the daemon loop (#23)"
)]
mod identity;
#[allow(
    dead_code,
    reason = "consumed by pairing (#7) and the daemon loop (#23)"
)]
mod link;
#[allow(dead_code, reason = "spawned per link by the daemon loop (#23)")]
mod link_driver;
#[allow(dead_code, reason = "consumed by pairing (#7) and the daemon loop")]
mod pin_store;
#[allow(
    dead_code,
    reason = "consumed by pairing (#7) and the daemon loop (#23)"
)]
mod tls;

use anyhow::{Context, Result};
use directories::ProjectDirs;

fn main() -> Result<()> {
    let dirs = ProjectDirs::from("", "", "apeirogon").context("no home directory found")?;
    let data_dir = dirs.data_local_dir();
    std::fs::create_dir_all(data_dir).context("cannot create the data directory")?;
    let identity = identity::load_or_create(data_dir)?;
    println!(
        "machine id {}",
        pin_store::hex_encode(identity.machine_id.as_bytes())
    );
    Ok(())
}
