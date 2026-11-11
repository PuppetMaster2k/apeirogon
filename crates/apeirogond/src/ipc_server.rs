// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::{ErrorKind, IpcConnection, IpcServer, Request, Response};
use a7n_platform::permissions::PermissionError;
use a7n_proto::limits;
use tokio::sync::{mpsc, oneshot};

use crate::command::{Command, Reply};
use crate::daemon::DaemonError;

pub(crate) async fn serve(server: IpcServer, commands: mpsc::Sender<Command>) {
    let mut connections = tokio::task::JoinSet::new();
    let accept = server.accept();
    tokio::pin!(accept);
    loop {
        tokio::select! {
            () = commands.closed() => break,
            result = &mut accept => {
                match result {
                    Ok(stream) => {
                        let commands = commands.clone();
                        connections.spawn(async move {
                            match IpcConnection::accept_stream(stream).await {
                                Ok(connection) => serve_connection(connection, commands).await,
                                Err(_) => crate::log::event!("kind=ipc_accept_failed"),
                            }
                        });
                    }
                    Err(_) => crate::log::event!("kind=ipc_accept_failed"),
                }
                accept.set(server.accept());
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
        Request::RecordHotkey { kind } => {
            execute(
                commands,
                |reply| Command::RecordHotkey { kind, reply },
                |value| Response::Recorded { value },
            )
            .await
        }
        Request::Status => execute(commands, Command::Status, Response::Status).await,
        clipboard @ (Request::ClipboardStatus
        | Request::ClipboardAccept { .. }
        | Request::ClipboardReject) => dispatch_clipboard(clipboard, commands).await,
        files @ (Request::SendFiles { .. }
        | Request::FilesAccept { .. }
        | Request::FilesReject { .. }
        | Request::CancelTransfer { .. }) => dispatch_files(files, commands).await,

        control @ (Request::Switch { .. }
        | Request::Wake { .. }
        | Request::SetPaused { .. }
        | Request::Connect { .. }) => dispatch_control(control, commands).await,
        settings @ (Request::ReloadConfig
        | Request::GetLayout
        | Request::SetLayout { .. }
        | Request::RequestPermission { .. }) => dispatch_settings(settings, commands).await,
        pairing @ (Request::PairHost | Request::PairJoin { .. } | Request::Unpair { .. }) => {
            dispatch_pairing(pairing, commands).await
        }
    }
}

async fn dispatch_control(request: Request, commands: &mpsc::Sender<Command>) -> Response {
    match request {
        Request::Switch { machine } => {
            execute(
                commands,
                |reply| Command::Switch { machine, reply },
                |name| name.map_or(Response::Done, |name| Response::Waking { name }),
            )
            .await
        }
        Request::Wake { machine } => {
            execute(
                commands,
                |reply| Command::Wake { machine, reply },
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
        Request::Connect { addr } => {
            execute(
                commands,
                |reply| Command::Connect { addr, reply },
                |()| Response::Done,
            )
            .await
        }
        _ => unreachable!("only control requests are dispatched here"),
    }
}

async fn dispatch_settings(request: Request, commands: &mpsc::Sender<Command>) -> Response {
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
        Request::RequestPermission { permission } => {
            let permission = permission.into();
            execute(
                commands,
                |reply| Command::RequestPermission { permission, reply },
                |()| Response::Done,
            )
            .await
        }

        _ => unreachable!("only settings requests are dispatched here"),
    }
}

async fn dispatch_pairing(request: Request, commands: &mpsc::Sender<Command>) -> Response {
    match request {
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

        _ => unreachable!("only pairing requests are dispatched here"),
    }
}

async fn dispatch_clipboard(request: Request, commands: &mpsc::Sender<Command>) -> Response {
    match request {
        Request::ClipboardStatus => {
            execute(commands, Command::ClipboardStatus, Response::Clipboard).await
        }
        Request::ClipboardAccept {
            machine,
            generation,
        } => {
            execute(
                commands,
                |reply| Command::ClipboardAccept {
                    machine,
                    generation,
                    reply,
                },
                |()| Response::Done,
            )
            .await
        }
        Request::ClipboardReject => {
            execute(commands, Command::ClipboardReject, |()| Response::Done).await
        }
        _ => unreachable!("only clipboard requests are dispatched here"),
    }
}

async fn dispatch_files(request: Request, commands: &mpsc::Sender<Command>) -> Response {
    match request {
        Request::SendFiles { peer, paths } => {
            execute(
                commands,
                |reply| Command::SendFiles { peer, paths, reply },
                |(offer_id, skipped_symlinks)| Response::FilesOffered {
                    offer_id,
                    skipped_symlinks,
                },
            )
            .await
        }
        Request::FilesAccept { peer, offer_id } => {
            execute(
                commands,
                |reply| Command::FilesAccept {
                    peer,
                    offer_id,
                    reply,
                },
                |()| Response::Done,
            )
            .await
        }
        Request::FilesReject { peer } => {
            execute(
                commands,
                |reply| Command::FilesReject { peer, reply },
                |()| Response::Done,
            )
            .await
        }
        Request::CancelTransfer { id } => {
            execute(
                commands,
                |reply| Command::CancelTransfer { id, reply },
                |()| Response::Done,
            )
            .await
        }
        _ => unreachable!("only file requests are dispatched here"),
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
        DaemonError::InvalidRequest(_)
        | DaemonError::Hotkey(_)
        | DaemonError::Permission(PermissionError::Unsupported) => ErrorKind::InvalidRequest,
        DaemonError::NotConnected => ErrorKind::NotConnected,
        DaemonError::NoSpace => ErrorKind::NoSpace,
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
