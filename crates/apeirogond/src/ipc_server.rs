// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::{ErrorKind, IpcConnection, IpcServer, Request, Response};
use a7n_proto::limits;
use tokio::sync::{mpsc, oneshot};

use crate::command::{Command, Reply};
use crate::daemon::DaemonError;

pub(crate) async fn serve(server: IpcServer, commands: mpsc::Sender<Command>) {
    loop {
        match server.accept().await {
            Ok(connection) => {
                tokio::spawn(serve_connection(connection, commands.clone()));
            }
            Err(_) => eprintln!("kind=ipc_accept_failed"),
        }
    }
}

async fn serve_connection(mut connection: IpcConnection, commands: mpsc::Sender<Command>) {
    while let Ok(request) = connection.read_request().await {
        let response = dispatch(request, &commands).await;
        if connection.write_response(&response).await.is_err() {
            break;
        }
    }
}

async fn dispatch(request: Request, commands: &mpsc::Sender<Command>) -> Response {
    match request {
        Request::ReloadConfig => {
            execute(commands, Command::ReloadConfig, |()| Response::Done).await
        }
        Request::GetLayout => execute(commands, Command::GetLayout, Response::Layout).await,
        Request::SetLayout { placed } => {
            execute(
                commands,
                |reply| Command::SetLayout { placed, reply },
                |()| Response::Done,
            )
            .await
        }
        Request::Status => execute(commands, Command::Status, Response::Status).await,
        Request::PairHost => {
            execute(commands, Command::PairHost, |code| Response::PairCode {
                code,
                expires_in_secs: limits::PAIRING_CODE_DURATION.as_secs(),
            })
            .await
        }
        Request::PairJoin { addr, code } => {
            execute(
                commands,
                |reply| Command::PairJoin { addr, code, reply },
                |entry| Response::Paired {
                    machine: entry.machine_id,
                    name: entry.name,
                },
            )
            .await
        }
        Request::Unpair { machine } => {
            execute(
                commands,
                |reply| Command::Unpair { machine, reply },
                |()| Response::Done,
            )
            .await
        }
        Request::Connect { addr } => {
            execute(
                commands,
                |reply| Command::Connect { addr, reply },
                |()| Response::Done,
            )
            .await
        }
    }
}

async fn execute<T>(
    commands: &mpsc::Sender<Command>,
    command: impl FnOnce(Reply<T>) -> Command,
    response: impl FnOnce(T) -> Response,
) -> Response {
    let (reply, receiver) = oneshot::channel();
    if commands.send(command(reply)).await.is_err() {
        return unavailable();
    }
    match receiver.await {
        Ok(Ok(value)) => response(value),
        Ok(Err(error)) => error_response(&error),
        Err(_) => unavailable(),
    }
}

fn unavailable() -> Response {
    Response::Error {
        kind: ErrorKind::Internal,
        message: "daemon command channel closed".into(),
    }
}

fn error_response(error: &DaemonError) -> Response {
    let kind = match error {
        DaemonError::Config(_) => ErrorKind::InvalidConfig,
        DaemonError::Layout(_) => ErrorKind::InvalidLayout,
        DaemonError::PairingBusy => ErrorKind::Busy,
        DaemonError::NotPaired => ErrorKind::NotPaired,
        DaemonError::Pairing(_) => ErrorKind::PairingRefused,
        _ => ErrorKind::Internal,
    };
    Response::Error {
        kind,
        message: if kind == ErrorKind::PairingRefused {
            "pairing refused".into()
        } else {
            error.to_string()
        },
    }
}

#[cfg(test)]
mod tests;
