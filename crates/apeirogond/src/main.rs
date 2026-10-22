// SPDX-License-Identifier: GPL-3.0-or-later
#![forbid(unsafe_code)]

#[allow(
    dead_code,
    reason = "key, cert and pin are consumed by TLS, which lands later"
)]
mod identity;
#[allow(dead_code, reason = "consumed by pairing and TLS, which land later")]
mod pin_store;

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
