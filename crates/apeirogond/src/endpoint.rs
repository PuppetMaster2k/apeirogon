// SPDX-License-Identifier: GPL-3.0-or-later

//! Binding the UDP endpoint, dialing and accepting links (PROTOCOL.md 1, 4).

use std::io;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;

use a7n_proto::limits;
use a7n_proto::v1::PeerInfo;
use quinn::{Endpoint, EndpointConfig, Incoming, ServerConfig, TokioRuntime};
use tokio::sync::{Semaphore, mpsc};
use tokio::time::timeout;

use crate::handshake;
use crate::link::{Link, LinkError};
use crate::tls::TLS_SERVER_NAME;

/// Binds `0.0.0.0:port`, or an ephemeral port if it is taken; read it from
/// `local_addr()` to advertise.
// ponytail: IPv4 only, since `[::]` is IPv6-only by default on Windows; bind a
// dual-stack socket via socket2 if IPv6-only LANs ever matter.
pub(crate) fn bind(port: u16, server_config: ServerConfig) -> io::Result<Endpoint> {
    bind_addr(
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)),
        server_config,
    )
}

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
/// The endpoint needs a default client config.
pub(crate) async fn dial(
    endpoint: &Endpoint,
    addr: SocketAddr,
    local: &PeerInfo,
) -> Result<Link, LinkError> {
    let handshake = async {
        let connection = endpoint.connect(addr, TLS_SERVER_NAME)?.await?;
        handshake::initiate(connection, local).await
    };
    timeout(limits::HANDSHAKE_TIMEOUT, handshake)
        .await
        .map_err(|_| LinkError::Timeout)?
}

/// Accepts connections until the endpoint closes, sending each established
/// link (or its failure) to `links`.
///
/// At most `UNAUTHENTICATED_SLOTS` handshakes run at once; further incoming
/// connections are refused.
pub(crate) async fn accept_loop(
    endpoint: Endpoint,
    local: PeerInfo,
    accepts_drive: bool,
    links: mpsc::Sender<Result<Link, LinkError>>,
) {
    let slots = Arc::new(Semaphore::new(limits::UNAUTHENTICATED_SLOTS));
    while let Some(incoming) = endpoint.accept().await {
        let Ok(permit) = Arc::clone(&slots).try_acquire_owned() else {
            incoming.refuse();
            continue;
        };
        let local = local.clone();
        let links = links.clone();
        tokio::spawn(async move {
            let result = respond_to(incoming, &local, accepts_drive).await;
            // The slot is free as soon as the handshake has an outcome.
            drop(permit);
            // The receiver is gone when the daemon shuts down.
            let _ = links.send(result).await;
        });
    }
}

async fn respond_to(
    incoming: Incoming,
    local: &PeerInfo,
    accepts_drive: bool,
) -> Result<Link, LinkError> {
    let handshake = async {
        let connection = incoming.await?;
        handshake::respond(connection, local, accepts_drive).await
    };
    timeout(limits::HANDSHAKE_TIMEOUT, handshake)
        .await
        .map_err(|_| LinkError::Timeout)?
}

#[cfg(test)]
mod loopback {
    use std::collections::HashSet;
    use std::net::Ipv4Addr;
    use std::sync::RwLock;

    use a7n_proto::v1::Capability;
    use quinn::ConnectionError;

    use super::*;
    use crate::identity::{self, Identity};
    use crate::link::Role;
    use crate::tls::{self, TrustedPins};

    struct Peer {
        info: PeerInfo,
        endpoint: Endpoint,
    }

    fn pins_of(trusted: &[&Identity]) -> TrustedPins {
        Arc::new(RwLock::new(
            trusted
                .iter()
                .map(|identity| identity.pin)
                .collect::<HashSet<_>>(),
        ))
    }

    fn peer(
        name: &str,
        capabilities: &[Capability],
        trusted: &[&Identity],
        own: &Identity,
    ) -> Peer {
        let pins = pins_of(trusted);
        let loopback = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let mut endpoint = bind_addr(
            loopback,
            tls::server_config(own, Arc::clone(&pins)).unwrap(),
        )
        .unwrap();
        endpoint.set_default_client_config(tls::client_config(own, pins).unwrap());
        let info = PeerInfo {
            name: name.to_owned(),
            capabilities: capabilities.iter().map(|&c| c as i32).collect(),
            ..PeerInfo::default()
        };
        Peer { info, endpoint }
    }

    fn new_identity() -> Identity {
        let dir = tempfile::tempdir().unwrap();
        identity::load_or_create(dir.path()).unwrap()
    }

    async fn run_pair(a: &Peer, b: &Peer) -> (Result<Link, LinkError>, Result<Link, LinkError>) {
        let addr = b.endpoint.local_addr().unwrap();
        let accept = async {
            let incoming = b.endpoint.accept().await.unwrap();
            respond_to(incoming, &b.info, true).await
        };
        tokio::join!(dial(&a.endpoint, addr, &a.info), accept)
    }

    #[tokio::test]
    async fn connect_paired_peers_both_see_each_others_machine_id() {
        // Arrange
        let (id_a, id_b) = (new_identity(), new_identity());
        let a = peer(
            "a",
            &[Capability::ClipboardText, Capability::Gestures],
            &[&id_b],
            &id_a,
        );
        let b = peer("b", &[Capability::ClipboardText], &[&id_a], &id_b);

        // Act
        let (dialed, accepted) = run_pair(&a, &b).await;
        let (dialed, accepted) = (dialed.unwrap(), accepted.unwrap());

        // Assert
        assert_eq!(dialed.peer_id, id_b.machine_id);
        assert_eq!(accepted.peer_id, id_a.machine_id);
        assert_eq!(
            (dialed.role, accepted.role),
            (Role::Initiator, Role::Responder)
        );
        assert_eq!(dialed.peer_info.name, "b");
        assert_eq!(dialed.capabilities, vec![Capability::ClipboardText]);
        assert_eq!(accepted.capabilities, vec![Capability::ClipboardText]);
    }

    #[tokio::test]
    async fn connect_unknown_pin_is_refused_with_bad_certificate() {
        // Arrange: b does not trust a.
        let (id_a, id_b) = (new_identity(), new_identity());
        let a = peer("a", &[], &[&id_b], &id_a);
        let b = peer("b", &[], &[], &id_b);

        // Act
        let (dialed, _) = run_pair(&a, &b).await;

        // Assert
        // The client finishes its handshake before the server checks its
        // certificate, so the alert arrives on the first stream operation.
        let Err(
            LinkError::Connection(lost) | LinkError::Read(quinn::ReadError::ConnectionLost(lost)),
        ) = dialed
        else {
            panic!("expected a lost connection, got {dialed:?}");
        };
        let ConnectionError::ConnectionClosed(close) = lost else {
            panic!("expected a close frame, got {lost:?}");
        };
        assert_eq!(
            u64::from(close.error_code),
            TLS_ALERT_BASE + BAD_CERTIFICATE_ALERT
        );
    }

    /// QUIC reports a TLS alert as crypto error `0x100 + alert` (RFC 9001 4.8).
    const TLS_ALERT_BASE: u64 = 0x100;
    /// TLS alert `bad_certificate` (RFC 8446 section 6.2).
    const BAD_CERTIFICATE_ALERT: u64 = 42;
}
