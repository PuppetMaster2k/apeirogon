// SPDX-License-Identifier: GPL-3.0-or-later
#![forbid(unsafe_code)]

use std::io::{self, Write};
use std::net::{IpAddr, SocketAddr};
use std::process::ExitCode;

use a7n_ipc::{
    DEFAULT_UDP_PORT, IpcClient, IpcError, MachineId, NearbyMachine, PairingCode, Request, Response,
};
use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

mod clip;
mod config;
mod daemon;
mod layout;
mod logs;

#[derive(Debug, Parser)]
#[command(version, about = "Control the apeirogon daemon")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create the initial config, refusing to overwrite an existing file.
    Init,
    /// Read or edit settings. Writes replace formatting and remove comments.
    #[command(
        after_help = "Setting values rewrites config.toml without preserving comments or formatting."
    )]
    Config {
        #[command(subcommand)]
        command: config::Command,
    },
    /// Arrange the desk's machines on an interactive canvas.
    Layout,
    /// Show this machine, its paired peers and nearby unpaired machines.
    Status,
    /// Show clipboard availability, current copy metadata and paired-peer policies.
    #[command(
        after_help = "Change a peer's policy with: a7n config set peers.<id>.clipboard <both|send|receive|off>"
    )]
    Clip,
    /// Show a pairing code, or join a target using a code read from stdin.
    Pair {
        /// Peer IP, IP:port, or nearby machine id / unique hex prefix (at least 4 digits).
        target: Option<String>,
    },
    /// Remove a paired machine by id, unique hex prefix (at least 4), or exact name.
    Unpair { machine: String },
    /// Move focus to a machine by id, unique hex prefix (at least 4), or exact name.
    Switch { machine: String },
    /// Manage the per-user daemon.
    Daemon {
        #[command(subcommand)]
        command: daemon::Command,
    },
    /// Read recent daemon events, even while the daemon is stopped.
    Logs {
        #[arg(short, long)]
        follow: bool,
        #[arg(short = 'n', default_value_t = 50)]
        lines: usize,
    },
    /// Dial a paired peer by address when automatic discovery is unavailable.
    Connect {
        /// Peer IP or IP:port; IPv6 ports use [IP]:port.
        #[arg(value_parser = parse_addr)]
        addr: SocketAddr,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run(Cli::parse().command).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(command: Command) -> Result<()> {
    let request = match command {
        Command::Daemon { command } => return daemon::run(command).await,
        Command::Logs { follow, lines } => return logs::run(lines, follow).await,
        Command::Switch { machine } => Request::Switch {
            machine: resolve_machine(&machine).await?,
        },
        Command::Init => return config::init(),
        Command::Config { command } => return config::run(command).await,
        Command::Layout => return layout::run().await,
        Command::Status => Request::Status,
        Command::Clip => Request::ClipboardStatus,
        Command::Pair { target: None } => Request::PairHost,
        Command::Pair {
            target: Some(target),
        } => Request::PairJoin {
            addr: resolve_pair_addr(&target).await?,
            code: read_code()?,
        },
        Command::Unpair { machine } => Request::Unpair {
            machine: resolve_machine(&machine).await?,
        },
        Command::Connect { addr } => Request::Connect { addr },
    };
    let mut client = connect_client().await?;
    let response = client.request(&request).await?;
    write_response(&mut io::stdout().lock(), &response)
}

async fn connect_client() -> Result<IpcClient> {
    IpcClient::connect().await.map_err(|error| match error {
        IpcError::Socket(_) => anyhow::anyhow!("apeirogond is not running"),
        error => error.into(),
    })
}

async fn resolve_pair_addr(target: &str) -> Result<SocketAddr> {
    if let Ok(addr) = parse_addr(target) {
        return Ok(addr);
    }
    let mut client = connect_client().await?;
    match client.request(&Request::Status).await? {
        Response::Status(status) => {
            resolve_target(target, &status.nearby).map_err(anyhow::Error::msg)
        }
        Response::Error { message, .. } => bail!("{message}"),
        _ => bail!("expected status from daemon"),
    }
}

fn resolve_target(target: &str, nearby: &[NearbyMachine]) -> Result<SocketAddr, &'static str> {
    const MIN_PREFIX_LENGTH: usize = 4;
    if let Ok(addr) = parse_addr(target) {
        return Ok(addr);
    }
    if target.len() < MIN_PREFIX_LENGTH
        || target.len() > MachineId::LEN * 2
        || !target.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("expected an IP address, IP:port, or machine id prefix of 4 to 32 hex digits");
    }
    let target = target.to_ascii_lowercase();
    let mut matches = nearby
        .iter()
        .filter(|machine| machine.id.to_string().starts_with(&target));
    let machine = matches
        .next()
        .ok_or("no nearby machine matches that id prefix")?;
    if matches.next().is_some() {
        return Err("ambiguous nearby machine id prefix; use more hex digits");
    }
    Ok(machine.addr)
}

async fn resolve_machine(target: &str) -> Result<MachineId> {
    let mut client = connect_client().await?;
    let Response::Status(status) = client.request(&Request::Status).await? else {
        bail!("expected status from daemon");
    };
    resolve_machine_id(
        target,
        std::iter::once((status.machine, status.name.as_str())).chain(
            status
                .peers
                .iter()
                .map(|peer| (peer.id, peer.name.as_str())),
        ),
    )
}

fn resolve_machine_id<'a>(
    target: &str,
    machines: impl Iterator<Item = (MachineId, &'a str)>,
) -> Result<MachineId> {
    let prefix = target.to_ascii_lowercase();
    let is_prefix = (4..=MachineId::LEN * 2).contains(&target.len())
        && target.bytes().all(|byte| byte.is_ascii_hexdigit());
    let mut matches = machines
        .filter(|(id, name)| *name == target || (is_prefix && id.to_string().starts_with(&prefix)));
    let (machine, _) = matches
        .next()
        .context("no machine matches that name or id prefix")?;
    if matches.next().is_some() {
        bail!("ambiguous machine name or id prefix; use the full id");
    }
    Ok(machine)
}

fn read_code() -> Result<PairingCode> {
    {
        let mut output = io::stdout().lock();
        write!(output, "Code: ").context("cannot write code prompt")?;
        output.flush().context("cannot write code prompt")?;
    }
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("cannot read pairing code")?;
    Ok(PairingCode::parse(input.trim())?)
}

fn parse_addr(input: &str) -> Result<SocketAddr, &'static str> {
    input
        .parse::<SocketAddr>()
        .or_else(|_| {
            input
                .parse::<IpAddr>()
                .map(|ip| SocketAddr::new(ip, DEFAULT_UDP_PORT))
        })
        .map_err(|_| "expected an IP address or IP:port (use [IPv6]:port for IPv6)")
}

fn write_response(output: &mut impl Write, response: &Response) -> Result<()> {
    match response {
        Response::Clipboard(info) => clip::write_status(output, info)?,
        Response::Status(status) => {
            writeln!(output, "{} {} (this machine)", status.name, status.machine)?;
            for peer in &status.peers {
                let state = if peer.connected {
                    "connected"
                } else {
                    "offline"
                };
                writeln!(output, "{} {} {state}", peer.name, peer.id)?;
            }
            if !status.nearby.is_empty() {
                writeln!(output, "nearby")?;
                for machine in &status.nearby {
                    writeln!(output, "{} {}", machine.id, machine.addr)?;
                }
            }
        }
        Response::PairCode {
            code,
            expires_in_secs,
        } => {
            writeln!(output, "{code} (valid for {expires_in_secs} s)")?;
        }
        Response::Paired { machine, name } => {
            writeln!(output, "Paired with {name} ({machine})")?;
        }
        Response::Done => {}
        Response::Layout(_) => bail!("unexpected layout response from daemon"),
        Response::Error { message, .. } => bail!("{message}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests;
