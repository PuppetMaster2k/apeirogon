// SPDX-License-Identifier: GPL-3.0-or-later

//! Binding the UDP endpoint, dialing and accepting links (PROTOCOL.md 1, 4).

use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;

use a7n_proto::limits;
use a7n_proto::v1::PeerInfo;
use quinn::{Endpoint, EndpointConfig, ServerConfig, TokioRuntime};
use tokio::sync::{Semaphore, mpsc};
use tokio::time::{Instant, timeout, timeout_at};

use crate::handshake;
use crate::link::{CLOSE_SUPERSEDED, Link, LinkError, peer_machine_id};
use crate::tls::TLS_SERVER_NAME;

/// Binds exactly `addr`, falling back to port 0 on that address if taken.
pub(crate) fn bind_addr(addr: SocketAddr, server_config: ServerConfig) -> io::Result<Endpoint> {
    let socket = match UdpSocket::bind(addr) {
        Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
            UdpSocket::bind(SocketAddr::new(addr.ip(), 0))?
        }
        other => other?,
    };
    endpoint_on(socket, server_config)
}

fn endpoint_on(socket: UdpSocket, server_config: ServerConfig) -> io::Result<Endpoint> {
    Endpoint::new(
        EndpointConfig::default(),
        Some(server_config),
        socket,
        Arc::new(TokioRuntime),
    )
}

/// Dials `addr` and runs the initiator handshake within `HANDSHAKE_TIMEOUT`.
/// The endpoint needs a default client config. Once TLS authenticates the peer,
/// `peer_id` identifies it even if the application handshake fails.
pub(crate) async fn dial(
    endpoint: &Endpoint,
    addr: SocketAddr,
    local: &PeerInfo,
    peer_id: &mut Option<a7n_proto::MachineId>,
) -> Result<Link, LinkError> {
    let handshake = async {
        let connection = endpoint.connect(addr, TLS_SERVER_NAME)?.await?;
        let peer = peer_machine_id(&connection)?;
        *peer_id = Some(peer);
        let local = handshake::local_info(&connection, endpoint, local);
        let result = handshake::initiate(connection.clone(), &local).await;
        if result.is_err()
            && matches!(connection.close_reason(),
            Some(quinn::ConnectionError::ApplicationClosed(close)) if close.error_code == CLOSE_SUPERSEDED)
        {
            return Err(LinkError::Superseded(peer));
        }
        result
    };
    timeout(limits::HANDSHAKE_TIMEOUT, handshake)
        .await
        .map_err(|_| LinkError::Timeout)?
}

/// Accept TLS on the shared port; keep a slot until ALPN dispatch and the
/// application handshake finish in the owner or its link handshake task.
pub(crate) async fn accept_loop(
    endpoint: Endpoint,
    connections: mpsc::Sender<(
        quinn::Connection,
        tokio::sync::OwnedSemaphorePermit,
        Instant,
    )>,
) {
    let slots = Arc::new(Semaphore::new(limits::UNAUTHENTICATED_SLOTS));
    while let Some(incoming) = endpoint.accept().await {
        let Ok(permit) = Arc::clone(&slots).try_acquire_owned() else {
            incoming.refuse();
            continue;
        };
        let connections = connections.clone();
        let deadline = Instant::now() + limits::HANDSHAKE_TIMEOUT;
        tokio::spawn(async move {
            if let Ok(Ok(connection)) = timeout_at(deadline, incoming).await {
                let _ = connections.send((connection, permit, deadline)).await;
            }
        });
    }
}

#[cfg(test)]
mod tests;
