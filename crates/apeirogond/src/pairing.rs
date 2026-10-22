// SPDX-License-Identifier: GPL-3.0-or-later

//! Pairing over a separate mutually authenticated QUIC connection (PROTOCOL.md 5).

use std::net::IpAddr;
use std::time::Instant;

use a7n_core::pairing::{AttemptLimiter, HostPairing, RequesterPairing};
use a7n_proto::v1::{
    PairAnswer, PairConfirm, PairDone, PairError, PairingMessage, PeerInfo, pairing_message::Body,
};
use a7n_proto::{FrameKind, Pin, decode_frame, encode_frame, limits};
use quinn::{Connection, RecvStream, SendStream};
use rand_core::{CryptoRng, RngCore};
use tokio::time::timeout;

use crate::identity;
use crate::link::{LinkError, peer_certificate};
use crate::pin_store::{Entry, PinStore, PinStoreError};

/// Domain separation for the SPAKE2 additional authenticated data.
const BINDING_LABEL: &[u8] = b"a7n-pair/1 binding";

/// Details stay local; the wire always carries a refusal without an explanation.
#[derive(Debug, thiserror::Error)]
pub(crate) enum PairingError {
    #[error("pairing refused")]
    Refused,
    #[error(transparent)]
    Link(#[from] LinkError),
    #[error(transparent)]
    Store(#[from] PinStoreError),
}

/// Dials pairing TLS within the handshake deadline; configure the endpoint with
/// `tls::pair_client_config` first.
pub(crate) async fn dial(
    endpoint: &quinn::Endpoint,
    addr: std::net::SocketAddr,
) -> Result<Connection, PairingError> {
    timeout(limits::HANDSHAKE_TIMEOUT, async {
        endpoint
            .connect(addr, crate::tls::TLS_SERVER_NAME)
            .map_err(LinkError::from)?
            .await
            .map_err(LinkError::from)
    })
    .await
    .map_err(|_| LinkError::Timeout)?
    .map_err(Into::into)
}

/// Accepts pairing TLS within the same deadline as a control handshake.
pub(crate) async fn accept(incoming: quinn::Incoming) -> Result<Connection, PairingError> {
    timeout(limits::HANDSHAKE_TIMEOUT, incoming)
        .await
        .map_err(|_| LinkError::Timeout)?
        .map_err(LinkError::from)
        .map_err(Into::into)
}

fn peer_pin(connection: &Connection) -> Result<Pin, PairingError> {
    identity::pin_of(&peer_certificate(connection)?)
        .map_err(LinkError::from)
        .map_err(Into::into)
}

fn binding(connection: &Connection) -> Result<[u8; 32], PairingError> {
    let mut ctx = [0; 32];
    connection
        .export_keying_material(&mut ctx, BINDING_LABEL, b"")
        .map_err(|_| PairingError::Refused)?;
    Ok(ctx)
}

async fn write_message(send: &mut SendStream, body: Body) -> Result<(), PairingError> {
    let mut buf = Vec::new();
    encode_frame(&PairingMessage { body: Some(body) }, &mut buf);
    timeout(limits::HANDSHAKE_TIMEOUT, send.write_all(&buf))
        .await
        .map_err(|_| LinkError::Timeout)?
        .map_err(LinkError::from)?;
    Ok(())
}

async fn read_message(recv: &mut RecvStream, buf: &mut Vec<u8>) -> Result<Body, PairingError> {
    timeout(limits::HANDSHAKE_TIMEOUT, async {
        loop {
            if let Some((message, used)) =
                decode_frame::<PairingMessage>(buf, FrameKind::Pairing).map_err(LinkError::from)?
            {
                buf.drain(..used);
                return message.body.ok_or(PairingError::Refused);
            }
            let chunk = recv
                .read_chunk(limits::CONTROL_FRAME_MAX, true)
                .await
                .map_err(LinkError::from)?
                .ok_or(LinkError::StreamClosed)?;
            buf.extend_from_slice(&chunk.bytes);
        }
    })
    .await
    .map_err(|_| LinkError::Timeout)?
}

async fn finish(send: &mut SendStream) -> Result<(), PairingError> {
    send.finish().map_err(|_| PairingError::Refused)?;
    // Delivery must precede connection close, particularly for PairDone.
    timeout(limits::HANDSHAKE_TIMEOUT, send.stopped())
        .await
        .map_err(|_| LinkError::Timeout)?
        .map_err(|_| PairingError::Refused)?;
    Ok(())
}

/// Runs one host exchange. The caller owns the code window, source limiter and
/// pin store across connections; time and randomness remain injectable.
pub(crate) async fn respond(
    connection: Connection,
    host: &mut HostPairing,
    info: &PeerInfo,
    limiter: &mut AttemptLimiter<IpAddr>,
    store: &mut PinStore,
    mut now: impl FnMut() -> Instant,
    rng: &mut (impl CryptoRng + RngCore),
) -> Result<Entry, PairingError> {
    let streams = timeout(limits::HANDSHAKE_TIMEOUT, connection.accept_bi()).await;
    let Ok(Ok((mut send, mut recv))) = streams else {
        host.refuse();
        connection.close(0u32.into(), b"");
        return Err(PairingError::Refused);
    };
    let mut failure = Body::Answer(PairAnswer {
        error: PairError::Refused.into(),
        ..PairAnswer::default()
    });
    let result = async {
        if !limiter.allow(connection.remote_address().ip(), now()) {
            return Err(PairingError::Refused);
        }
        let pin = peer_pin(&connection)?;
        let ctx = binding(&connection)?;
        let mut buf = Vec::new();
        let Body::Start(start) = read_message(&mut recv, &mut buf).await? else {
            return Err(PairingError::Refused);
        };
        let answer = host
            .start(start, pin, ctx, info.clone(), now(), rng)
            .map_err(|_| PairingError::Refused)?;
        write_message(&mut send, Body::Answer(answer)).await?;
        failure = Body::Done(PairDone {
            error: PairError::Refused.into(),
            ..PairDone::default()
        });
        let Body::Confirm(confirm) = read_message(&mut recv, &mut buf).await? else {
            return Err(PairingError::Refused);
        };
        let paired = host
            .confirm(confirm, now())
            .map_err(|_| PairingError::Refused)?;
        let entry = store_peer(
            store,
            &connection,
            paired.peer.pin,
            paired.peer.info.name.clone(),
        )?;
        write_message(&mut send, Body::Done(paired.done())).await?;
        finish(&mut send).await?;
        Ok(entry)
    }
    .await;
    if result.is_err() {
        host.refuse();
        refuse(&mut send, failure).await;
    }
    connection.close(0u32.into(), b"");
    result
}

/// Runs the requester exchange, persisting only after the host's storage proof.
pub(crate) async fn initiate(
    connection: Connection,
    mut requester: RequesterPairing,
    store: &mut PinStore,
) -> Result<Entry, PairingError> {
    let streams = timeout(limits::HANDSHAKE_TIMEOUT, connection.open_bi()).await;
    let Ok(Ok((mut send, mut recv))) = streams else {
        connection.close(0u32.into(), b"");
        return Err(PairingError::Refused);
    };
    let result = async {
        let pin = peer_pin(&connection)?;
        let ctx = binding(&connection)?;
        write_message(&mut send, Body::Start(requester.start())).await?;
        let mut buf = Vec::new();
        let Body::Answer(answer) = read_message(&mut recv, &mut buf).await? else {
            return Err(PairingError::Refused);
        };
        let confirm = requester
            .answer(answer, pin, ctx)
            .map_err(|_| PairingError::Refused)?;
        write_message(&mut send, Body::Confirm(confirm)).await?;
        let Body::Done(done) = read_message(&mut recv, &mut buf).await? else {
            return Err(PairingError::Refused);
        };
        let peer = requester.done(done).map_err(|_| PairingError::Refused)?;
        let entry = store_peer(store, &connection, peer.pin, peer.info.name)?;
        Ok(entry)
    }
    .await;
    if result.is_err() {
        refuse(
            &mut send,
            Body::Confirm(PairConfirm {
                error: PairError::Refused.into(),
                ..PairConfirm::default()
            }),
        )
        .await;
    }
    connection.close(0u32.into(), b"");
    result
}

async fn refuse(send: &mut SendStream, body: Body) {
    if write_message(send, body).await.is_ok() {
        let _ = finish(send).await;
    }
}

/// Saves a verified peer and restores the in-memory store if persistence fails.
fn store_peer(
    store: &mut PinStore,
    connection: &Connection,
    pin: Pin,
    name: String,
) -> Result<Entry, PairingError> {
    let machine_id = crate::link::peer_machine_id(connection)?;
    let entry = Entry {
        machine_id,
        pin,
        name,
    };
    let previous = store.get(&machine_id).cloned();
    store.add(entry.clone());
    if let Err(error) = store.save() {
        store.remove(&machine_id);
        if let Some(previous) = previous {
            store.add(previous);
        }
        return Err(error.into());
    }
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr};

    use quinn::Endpoint;
    use tempfile::TempDir;

    use super::*;
    use crate::endpoint;
    use crate::identity::Identity;
    use crate::pin_store::PinStore;
    use crate::tls;
    use a7n_core::pairing::PairingCode;
    use rand_core::OsRng;

    struct Peer {
        identity: Identity,
        endpoint: Endpoint,
        store: PinStore,
        info: PeerInfo,
        dir: TempDir,
    }

    impl Peer {
        fn new(name: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let identity = identity::load_or_create(dir.path()).unwrap();
            let store = PinStore::load(&dir.path().join("pins.toml")).unwrap();
            let mut endpoint = endpoint::bind_addr(
                SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
                tls::pair_server_config(&identity).unwrap(),
            )
            .unwrap();
            endpoint.set_default_client_config(tls::pair_client_config(&identity).unwrap());
            Self {
                identity,
                endpoint,
                store,
                info: PeerInfo {
                    name: name.into(),
                    ..PeerInfo::default()
                },
                dir,
            }
        }
    }

    async fn connect(a: &Peer, b: &Peer) -> (Connection, Connection) {
        let connect = dial(&a.endpoint, b.endpoint.local_addr().unwrap());
        let incoming = async { accept(b.endpoint.accept().await.unwrap()).await };
        let (client, server) = tokio::join!(connect, incoming);
        (client.unwrap(), server.unwrap())
    }
    #[tokio::test]
    async fn pair_tls_unknown_certificates_authenticate_and_bind() {
        // Arrange
        let a = Peer::new("a");
        let b = Peer::new("b");

        // Act
        let (client, server) = connect(&a, &b).await;

        // Assert
        assert_eq!(peer_pin(&client).unwrap(), b.identity.pin);
        assert_eq!(peer_pin(&server).unwrap(), a.identity.pin);
        assert_eq!(binding(&client).unwrap(), binding(&server).unwrap());
    }

    #[tokio::test]
    async fn store_peer_save_failure_restores_previous_entry() {
        // Arrange
        let a = Peer::new("a");
        let b = Peer::new("b");
        let (client, _server) = connect(&a, &b).await;
        let mut store = PinStore::load(&a.dir.path().join("missing/pins.toml")).unwrap();
        let previous = crate::pin_store::Entry {
            machine_id: b.identity.machine_id,
            pin: a.identity.pin,
            name: "previous".into(),
        };
        store.add(previous.clone());

        // Act
        let result = store_peer(&mut store, &client, b.identity.pin, "new".into());

        // Assert
        assert!(result.is_err());
        assert_eq!(store.get(&b.identity.machine_id), Some(&previous));
    }

    fn requester(peer: &Peer, code: &str) -> RequesterPairing {
        RequesterPairing::new(
            PairingCode::parse(code).unwrap(),
            None,
            peer.identity.pin,
            peer.info.clone(),
            &mut OsRng,
        )
    }

    async fn run_pair(
        a: &mut Peer,
        b: &mut Peer,
        requester_code: &str,
        host_code: &str,
        opened_at: Instant,
        now: Instant,
    ) -> (Result<Entry, PairingError>, Result<Entry, PairingError>) {
        let (client, server) = connect(a, b).await;
        let requester = requester(a, requester_code);
        let mut host = HostPairing::new(
            PairingCode::parse(host_code).unwrap(),
            None,
            b.identity.pin,
            opened_at,
        );
        let mut limiter = AttemptLimiter::new(limits::UNAUTHENTICATED_SLOTS);
        let mut rng = OsRng;
        tokio::join!(
            initiate(client, requester, &mut a.store),
            respond(
                server,
                &mut host,
                &b.info,
                &mut limiter,
                &mut b.store,
                || now,
                &mut rng
            )
        )
    }

    fn assert_stored(a: &Peer, b: &Peer) {
        let reloaded = PinStore::load(&a.dir.path().join("pins.toml")).unwrap();
        let entry = reloaded.get(&b.identity.machine_id).unwrap();
        assert_eq!(entry.pin, b.identity.pin);
        assert_eq!(entry.name, b.info.name);
    }

    fn assert_empty(a: &Peer, b: &Peer) {
        assert!(a.store.pins().next().is_none());
        assert!(b.store.pins().next().is_none());
        for peer in [a, b] {
            let reloaded = PinStore::load(&peer.dir.path().join("pins.toml")).unwrap();
            assert!(reloaded.pins().next().is_none());
        }
    }

    #[tokio::test]
    async fn pair_correct_code_persists_both_peers() {
        // Arrange
        let mut a = Peer::new("requester");
        let mut b = Peer::new("host");
        let now = std::time::Instant::now();

        // Act
        let (requester, host) = run_pair(&mut a, &mut b, "123456", "123456", now, now).await;

        // Assert
        assert!(requester.is_ok(), "{requester:?}");
        assert!(host.is_ok(), "{host:?}");
        assert_stored(&a, &b);
        assert_stored(&b, &a);
    }

    #[tokio::test]
    async fn pair_host_save_failure_stores_nothing() {
        // Arrange
        let mut a = Peer::new("requester");
        let mut b = Peer::new("host");
        b.store = PinStore::load(&b.dir.path().join("missing/pins.toml")).unwrap();
        let now = Instant::now();

        // Act
        let (requester, host) = run_pair(&mut a, &mut b, "123456", "123456", now, now).await;

        // Assert
        assert!(requester.is_err());
        assert!(matches!(host, Err(PairingError::Store(_))));
        assert_empty(&a, &b);
    }

    #[tokio::test]
    async fn pair_wrong_code_stores_nothing() {
        // Arrange
        let mut a = Peer::new("requester");
        let mut b = Peer::new("host");
        let now = std::time::Instant::now();

        // Act
        let (requester, host) = run_pair(&mut a, &mut b, "654321", "123456", now, now).await;

        // Assert
        assert!(requester.is_err());
        assert!(host.is_err());
        assert_empty(&a, &b);
    }

    #[tokio::test]
    async fn pair_expired_code_stores_nothing() {
        // Arrange
        let mut a = Peer::new("requester");
        let mut b = Peer::new("host");
        let opened_at = std::time::Instant::now();
        let now = opened_at + limits::PAIRING_CODE_DURATION;

        // Act
        let (requester, host) = run_pair(&mut a, &mut b, "123456", "123456", opened_at, now).await;

        // Assert
        assert!(requester.is_err());
        assert!(host.is_err());
        assert_empty(&a, &b);
    }

    async fn relay(front: Connection, back: Connection) {
        async fn copy(mut recv: RecvStream, mut send: SendStream) -> Result<(), PairingError> {
            while let Some(chunk) = timeout(
                limits::HANDSHAKE_TIMEOUT,
                recv.read_chunk(limits::CONTROL_FRAME_MAX, true),
            )
            .await
            .map_err(|_| LinkError::Timeout)?
            .map_err(LinkError::from)?
            {
                timeout(limits::HANDSHAKE_TIMEOUT, send.write_all(&chunk.bytes))
                    .await
                    .map_err(|_| LinkError::Timeout)?
                    .map_err(LinkError::from)?;
            }
            finish(&mut send).await
        }
        let (front_send, front_recv) = timeout(limits::HANDSHAKE_TIMEOUT, front.accept_bi())
            .await
            .unwrap()
            .unwrap();
        let (back_send, back_recv) = back.open_bi().await.unwrap();
        let _ = tokio::join!(copy(front_recv, back_send), copy(back_recv, front_send));
    }

    #[tokio::test]
    async fn pair_terminating_relay_stores_nothing() {
        // Arrange
        let mut a = Peer::new("requester");
        let mut b = Peer::new("host");
        let relay_peer = Peer::new("relay");
        let (client, front) = connect(&a, &relay_peer).await;
        let (back, server) = connect(&relay_peer, &b).await;
        let requester = requester(&a, "123456");
        let now = Instant::now();
        let mut host = HostPairing::new(
            PairingCode::parse("123456").unwrap(),
            None,
            b.identity.pin,
            now,
        );
        let mut limiter = AttemptLimiter::new(limits::UNAUTHENTICATED_SLOTS);
        let mut rng = OsRng;

        // Act
        let (requester, host, ()) = tokio::join!(
            initiate(client, requester, &mut a.store),
            respond(
                server,
                &mut host,
                &b.info,
                &mut limiter,
                &mut b.store,
                || now,
                &mut rng
            ),
            relay(front, back)
        );

        // Assert
        assert!(requester.is_err());
        assert!(host.is_err());
        assert_empty(&a, &b);
    }
}
