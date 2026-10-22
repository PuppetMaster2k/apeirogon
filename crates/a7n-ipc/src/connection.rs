// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_platform::local_socket::{self, Listener, Stream};
use a7n_proto::limits;
use interprocess::local_socket::tokio::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{
    ErrorKind, IPC_VERSION, IpcError, Request, Response, SocketLocation, framing::JsonLines,
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Hello {
    ipc_version: u32,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum HelloReply {
    Hello(Hello),
    Response(Response),
}

/// Listener for local CLI and GUI connections.
#[derive(Debug)]
pub struct IpcServer {
    listener: Listener,
}

impl IpcServer {
    /// Bind the default per-user location.
    ///
    /// # Errors
    /// Returns endpoint security or OS errors.
    pub async fn bind() -> Result<Self, IpcError> {
        Self::bind_at(&local_socket::default_location()?).await
    }

    /// Bind an explicit per-user location (also useful for isolated instances).
    ///
    /// # Errors
    /// Returns endpoint security or OS errors, including an already running daemon.
    pub async fn bind_at(location: &SocketLocation) -> Result<Self, IpcError> {
        Ok(Self {
            listener: local_socket::bind_user_only(location).await?,
        })
    }

    /// Accept and negotiate one connection. A failed client's Hello does not close the listener.
    ///
    /// # Errors
    /// Returns accept, timeout, framing or version errors.
    pub async fn accept(&self) -> Result<IpcConnection, IpcError> {
        IpcConnection::accept_stream(self.listener.accept().await?).await
    }
}

/// Server side of a negotiated connection, with one response per request.
#[derive(Debug)]
pub struct IpcConnection<S = Stream> {
    lines: JsonLines<S>,
}

impl<S: AsyncRead + AsyncWrite + Unpin> IpcConnection<S> {
    async fn accept_stream(stream: S) -> Result<Self, IpcError> {
        let mut lines = JsonLines::new(stream);
        tokio::time::timeout(limits::HANDSHAKE_TIMEOUT, async {
            let hello: Hello = lines.read().await?;
            if hello.ipc_version != IPC_VERSION {
                lines
                    .write(&Response::Error {
                        kind: ErrorKind::UnsupportedVersion,
                        message: format!(
                            "unsupported IPC version {}; expected {IPC_VERSION}",
                            hello.ipc_version
                        ),
                    })
                    .await?;
                return Err(IpcError::UnsupportedVersion {
                    received: hello.ipc_version,
                    expected: IPC_VERSION,
                });
            }
            lines
                .write(&Hello {
                    ipc_version: IPC_VERSION,
                })
                .await
        })
        .await
        .map_err(|_| IpcError::Timeout)??;
        Ok(Self { lines })
    }

    /// Read the next command. Errors close the connection.
    ///
    /// # Errors
    /// Returns timeout, EOF, oversized-line or invalid-request errors.
    pub async fn read_request(&mut self) -> Result<Request, IpcError> {
        self.lines.read().await
    }

    /// Reply to the last command. Errors close the connection.
    ///
    /// # Errors
    /// Returns timeout, serialization, oversized-line or OS errors.
    pub async fn write_response(&mut self, response: &Response) -> Result<(), IpcError> {
        self.lines.write(response).await
    }
}

/// Sequential local API client. Application errors are returned as `Response::Error`.
#[derive(Debug)]
pub struct IpcClient<S = Stream> {
    lines: JsonLines<S>,
}

impl IpcClient {
    /// Connect to the default daemon and negotiate its API version.
    ///
    /// # Errors
    /// Returns connection or handshake errors.
    pub async fn connect() -> Result<Self, IpcError> {
        Self::connect_at(&local_socket::default_location()?).await
    }

    /// Connect to an explicit location and negotiate its API version.
    ///
    /// # Errors
    /// Returns connection or handshake errors.
    pub async fn connect_at(location: &SocketLocation) -> Result<Self, IpcError> {
        Self::connect_stream(local_socket::connect(location).await?).await
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> IpcClient<S> {
    async fn connect_stream(stream: S) -> Result<Self, IpcError> {
        let mut lines = JsonLines::new(stream);
        tokio::time::timeout(limits::HANDSHAKE_TIMEOUT, async {
            lines
                .write(&Hello {
                    ipc_version: IPC_VERSION,
                })
                .await?;
            match lines.read().await? {
                HelloReply::Hello(Hello {
                    ipc_version: IPC_VERSION,
                }) => Ok(()),
                HelloReply::Hello(hello) => Err(IpcError::UnsupportedVersion {
                    received: hello.ipc_version,
                    expected: IPC_VERSION,
                }),
                HelloReply::Response(Response::Error { kind, message }) => {
                    Err(IpcError::HandshakeRefused { kind, message })
                }
                HelloReply::Response(_) => Err(IpcError::ExpectedHello),
            }
        })
        .await
        .map_err(|_| IpcError::Timeout)??;
        Ok(Self { lines })
    }

    /// Send one command and read its response. Any transport error closes the connection.
    ///
    /// # Errors
    /// Returns timeout, framing, serialization or OS errors.
    pub async fn request(&mut self, request: &Request) -> Result<Response, IpcError> {
        self.lines.write(request).await?;
        self.lines.read().await
    }
}

#[cfg(test)]
mod tests;
