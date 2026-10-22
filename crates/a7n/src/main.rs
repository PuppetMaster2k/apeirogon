// SPDX-License-Identifier: GPL-3.0-or-later
#![forbid(unsafe_code)]

use std::io::{self, Write};
use std::net::{IpAddr, SocketAddr};
use std::process::ExitCode;

use a7n_ipc::{DEFAULT_UDP_PORT, IpcClient, IpcError, MachineId, PairingCode, Request, Response};
use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(version, about = "Control the apeirogon daemon")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show this machine and its paired peers.
    Status,
    /// Show a pairing code, or join an address using a code read from stdin.
    Pair {
        /// Peer IP or IP:port; IPv6 ports use [IP]:port.
        #[arg(value_parser = parse_addr)]
        addr: Option<SocketAddr>,
    },
    /// Remove a paired machine by its 32-hex machine id.
    Unpair {
        #[arg(value_parser = parse_machine)]
        machine: MachineId,
    },
    /// Dial a paired peer; temporary until automatic discovery (#9).
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
        Command::Status => Request::Status,
        Command::Pair { addr: None } => Request::PairHost,
        Command::Pair { addr: Some(addr) } => Request::PairJoin {
            addr,
            code: read_code()?,
        },
        Command::Unpair { machine } => Request::Unpair { machine },
        Command::Connect { addr } => Request::Connect { addr },
    };
    let mut client = IpcClient::connect().await.map_err(|error| match error {
        IpcError::Socket(_) => anyhow::anyhow!("apeirogond is not running"),
        error => error.into(),
    })?;
    let response = client.request(&request).await?;
    write_response(&mut io::stdout().lock(), &response)
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

fn parse_machine(input: &str) -> Result<MachineId, &'static str> {
    const INVALID_ID: &str = "machine id must contain 32 hex digits";
    if input.len() != MachineId::LEN * 2 || !input.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(INVALID_ID);
    }
    let mut bytes = [0; MachineId::LEN];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&input[index * 2..index * 2 + 2], 16).map_err(|_| INVALID_ID)?;
    }
    MachineId::try_from(bytes.as_slice()).map_err(|_| INVALID_ID)
}

fn write_machine(output: &mut impl Write, machine: MachineId) -> io::Result<()> {
    for byte in machine.as_bytes() {
        write!(output, "{byte:02x}")?;
    }
    Ok(())
}

fn write_response(output: &mut impl Write, response: &Response) -> Result<()> {
    match response {
        Response::Status(status) => {
            write!(output, "{} ", status.name)?;
            write_machine(output, status.machine)?;
            writeln!(output, " (this machine)")?;
            for peer in &status.peers {
                write!(output, "{} ", peer.name)?;
                write_machine(output, peer.id)?;
                let state = if peer.connected {
                    "connected"
                } else {
                    "offline"
                };
                writeln!(output, " {state}")?;
            }
        }
        Response::PairCode {
            code,
            expires_in_secs,
        } => {
            writeln!(output, "{code} (valid for {expires_in_secs} s)")?;
        }
        Response::Paired { machine, name } => {
            write!(output, "Paired with {name} (")?;
            write_machine(output, *machine)?;
            writeln!(output, ")")?;
        }
        Response::Done => {}
        Response::Error { message, .. } => bail!("{message}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests;
