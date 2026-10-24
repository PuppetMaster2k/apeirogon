// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared daemon startup for local clients.
use std::time::Duration;

use a7n_platform::autostart;

use crate::{IpcClient, Request, Response};

const START_TIMEOUT: Duration = Duration::from_secs(3);
const START_POLL: Duration = Duration::from_millis(50);

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("cannot start apeirogond: {0}")]
    Spawn(#[from] std::io::Error),
    #[error(
        "apeirogond did not become ready within 3 s; it may be waiting for desktop permissions; check a7n logs"
    )]
    Timeout,
}

/// Return the running daemon's process id, if reachable.
pub async fn status() -> Option<u32> {
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

/// Start the daemon if necessary and wait for its IPC endpoint.
/// # Errors
/// Returns spawn errors or a readiness timeout.
pub async fn start() -> Result<u32, StartError> {
    if let Some(pid) = status().await {
        return Ok(pid);
    }
    autostart::start(&autostart::daemon_executable()?)?;
    tokio::time::timeout(START_TIMEOUT, async {
        loop {
            if let Some(pid) = status().await {
                return pid;
            }
            tokio::time::sleep(START_POLL).await;
        }
    })
    .await
    .map_err(|_| StartError::Timeout)
}
