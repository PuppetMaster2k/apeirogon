// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use a7n_ipc::{
    FileTransfer, HeldFiles, IpcClient, MachineId, Request, Response, StatusInfo, TransferDirection,
};
use anyhow::{Context, Result, bail};

use super::{connect_client, resolve_machine};

#[derive(Debug, clap::Subcommand)]
pub(super) enum Command {
    /// Accept the file offer a peer is holding for confirmation.
    Accept {
        /// Peer name, machine id or unique hex prefix (at least 4 digits).
        peer: String,
    },
    /// Discard the file offer a peer is holding for confirmation.
    Reject {
        /// Peer name, machine id or unique hex prefix (at least 4 digits).
        peer: String,
    },
    /// Cancel an active transfer by the id shown in `a7n status`.
    Cancel { id: u64 },
}

pub(super) async fn run(command: Command) -> Result<()> {
    let request = match command {
        Command::Accept { peer } => Request::FilesAccept {
            peer: resolve_machine(&peer).await?,
        },
        Command::Reject { peer } => Request::FilesReject {
            peer: resolve_machine(&peer).await?,
        },
        Command::Cancel { id } => Request::CancelTransfer { id },
    };
    expect_done(connect_client().await?.request(&request).await?)
}

fn expect_done(response: Response) -> Result<()> {
    match response {
        Response::Done => Ok(()),
        Response::Error { message, .. } => bail!("{message}"),
        _ => bail!("unexpected response from daemon"),
    }
}

const KIBIBYTE: u64 = 1024;
const SIZE_UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
const TENTHS: u128 = 10;

/// Binary units with one decimal, using integer math so nothing is rounded up.
pub(super) fn human_size(bytes: u64) -> String {
    if bytes < KIBIBYTE {
        return format!("{bytes} B");
    }
    let mut tenths = u128::from(bytes) * TENTHS;
    let mut unit = "";
    for next in SIZE_UNITS {
        tenths /= u128::from(KIBIBYTE);
        unit = next;
        if tenths < u128::from(KIBIBYTE) * TENTHS {
            break;
        }
    }
    format!("{}.{} {unit}", tenths / TENTHS, tenths % TENTHS)
}

fn peer_label(status: &StatusInfo, id: MachineId) -> String {
    status
        .peers
        .iter()
        .find(|peer| peer.id == id)
        .map_or_else(|| id.to_string(), |peer| peer.name.clone())
}

fn direction_label(direction: TransferDirection) -> &'static str {
    match direction {
        TransferDirection::Send => "send",
        TransferDirection::Receive => "receive",
    }
}

pub(super) fn write_held(
    output: &mut impl Write,
    status: &StatusInfo,
    held: &HeldFiles,
) -> io::Result<()> {
    writeln!(
        output,
        "files waiting for confirmation: {} wants to send {} files, {} (a7n files accept {})",
        peer_label(status, held.peer),
        held.files,
        human_size(held.total_size),
        held.peer,
    )
}

pub(super) fn write_transfer(
    output: &mut impl Write,
    status: &StatusInfo,
    transfer: &FileTransfer,
) -> io::Result<()> {
    write!(
        output,
        "transfer {} {} {}: ",
        transfer.id,
        direction_label(transfer.direction),
        peer_label(status, transfer.peer),
    )?;
    write_progress(output, transfer)
}

fn write_progress(output: &mut impl Write, transfer: &FileTransfer) -> io::Result<()> {
    writeln!(
        output,
        "{}/{} files, {} / {}",
        transfer.files_done,
        transfer.files_total,
        human_size(transfer.bytes_done),
        human_size(transfer.bytes_total),
    )
}

const POLL_INTERVAL: Duration = Duration::from_millis(250);
/// The daemon holds an unanswered offer for 60 s (`ASK_TIMEOUT` in apeirogond);
/// the margin covers connection setup before the receiver starts fetching.
const OFFER_WAIT: Duration = Duration::from_secs(65);

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Poll<'a> {
    Progress(&'a FileTransfer),
    Finished,
    Waiting,
}

/// A send transfer only appears once the peer starts fetching, so the offer
/// cannot be correlated by id; the first send transfer to `peer` is tracked by
/// its transfer id from then on.
pub(super) fn poll_transfer(
    transfers: &[FileTransfer],
    peer: MachineId,
    tracked: Option<u64>,
) -> Poll<'_> {
    let found = transfers.iter().find(|transfer| match tracked {
        Some(id) => transfer.id == id,
        None => transfer.peer == peer && transfer.direction == TransferDirection::Send,
    });
    match (found, tracked) {
        (Some(transfer), _) => Poll::Progress(transfer),
        (None, Some(_)) => Poll::Finished,
        (None, None) => Poll::Waiting,
    }
}

pub(super) async fn send(target: &str, paths: &[PathBuf], wait: bool) -> Result<()> {
    let peer = resolve_machine(target).await?;
    let paths = paths
        .iter()
        .map(|path| absolute_path(path))
        .collect::<Result<Vec<_>>>()?;
    let mut client = connect_client().await?;
    let response = client.request(&Request::SendFiles { peer, paths }).await?;
    let (offer_id, skipped_symlinks) = match response {
        Response::FilesOffered {
            offer_id,
            skipped_symlinks,
        } => (offer_id, skipped_symlinks),
        Response::Error { message, .. } => bail!("{message}"),
        _ => bail!("unexpected response from daemon"),
    };
    let mut output = io::stdout().lock();
    writeln!(output, "offered files (offer {offer_id})")?;
    if skipped_symlinks > 0 {
        writeln!(output, "skipped {skipped_symlinks} symlinks")?;
    }
    if !wait {
        return Ok(());
    }
    writeln!(output, "waiting for the peer to accept")?;
    follow_send(&mut client, &mut output, peer).await
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(path).with_context(|| format!("cannot read {}", path.display()))
}

async fn follow_send(
    client: &mut IpcClient,
    output: &mut impl Write,
    peer: MachineId,
) -> Result<()> {
    let started = Instant::now();
    let mut tracked = None;
    let mut last_shown = None;
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        let Response::Status(status) = client.request(&Request::Status).await? else {
            bail!("expected status from daemon");
        };
        match poll_transfer(&status.transfers, peer, tracked) {
            Poll::Progress(transfer) => {
                tracked = Some(transfer.id);
                if last_shown.as_ref() != Some(transfer) {
                    write_progress(output, transfer)?;
                    last_shown = Some(transfer.clone());
                }
            }
            Poll::Finished => {
                writeln!(output, "transfer finished")?;
                return Ok(());
            }
            Poll::Waiting if started.elapsed() > OFFER_WAIT => bail!(
                "the peer did not start the transfer (offer declined, expired, or already finished)"
            ),
            Poll::Waiting => {}
        }
    }
}
