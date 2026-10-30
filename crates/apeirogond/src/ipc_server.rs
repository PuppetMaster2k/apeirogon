// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::{ErrorKind, IpcConnection, IpcServer, Request, Response};
use a7n_proto::limits;
use tokio::sync::{mpsc, oneshot};

use crate::command::{Command, Reply};
use crate::daemon::DaemonError;

pub(crate) async fn serve(server: IpcServer, commands: mpsc::Sender<Command>) {
    let mut connections = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            () = commands.closed() => break,
            result = server.accept() => match result {
                Ok(connection) => { connections.spawn(serve_connection(connection, commands.clone())); }
                Err(_) => crate::log::event!("kind=ipc_accept_failed"),
            },
            Some(_) = connections.join_next(), if !connections.is_empty() => {},
        }
    }
    while connections.join_next().await.is_some() {}
}

async fn serve_connection(mut connection: IpcConnection, commands: mpsc::Sender<Command>) {
    loop {
        let request = tokio::select! {
            () = commands.closed() => break,
            request = connection.read_request() => match request { Ok(request) => request, Err(_) => break },
        };
        let shutdown = matches!(request, Request::Shutdown);
        let response = dispatch(request, &commands).await;
        if connection.write_response(&response).await.is_err() || shutdown {
            break;
        }
    }
}

async fn dispatch(request: Request, commands: &mpsc::Sender<Command>) -> Response {
    match request {
        Request::Shutdown => execute(commands, Command::Shutdown, |()| Response::Done).await,
        Request::Switch { machine } => {
            execute(
                commands,
                |reply| Command::Switch { machine, reply },
                |()| Response::Done,
            )
            .await
        }
        Request::SetPaused { paused } => {
            execute(
                commands,
                |reply| Command::SetPaused { paused, reply },
                |()| Response::Done,
            )
            .await
        }
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
        Request::ClipboardStatus => {
            execute(commands, Command::ClipboardStatus, Response::Clipboard).await
        }
        Request::ClipboardAccept => {
            execute(commands, Command::ClipboardAccept, |()| Response::Done).await
        }
        Request::ClipboardReject => {
            execute(commands, Command::ClipboardReject, |()| Response::Done).await
        }
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
        DaemonError::PairingBusy | DaemonError::Paused => ErrorKind::Busy,
        DaemonError::InvalidRequest(_) => ErrorKind::InvalidRequest,
        DaemonError::NotConnected => ErrorKind::NotConnected,
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
