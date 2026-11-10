// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashSet;
use std::net::Ipv4Addr;

use quinn::Connection;
use tokio::sync::mpsc;
use tokio::time::timeout;

use super::test_peer::Peer;
use super::*;
use crate::pairing::{self as exchange, PairingError};

#[tokio::test]
async fn pair_join_pin_write_failure_preserves_previous_trust() {
    // Arrange
    let host = Peer::start();
    let host_identity = identity::load_or_create(host.dir.path()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut daemon = Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    let previous = daemon.identity.pin;
    daemon.pins = PinStore::load(&dir.path().join("missing/pins.toml")).unwrap();
    daemon.pins.add(Entry {
        machine_id: host.machine,
        pin: previous,
        name: "previous".into(),
        spki_der: None,
        hw_addr: None,
        screen_mm: None,
    });
    daemon.sync_trust().unwrap();
    let trust = Arc::clone(&daemon.trust);
    let requester = Peer::run(daemon, dir);
    let code = host.command(Command::PairHost).await.unwrap();

    // Act
    let result = requester
        .command(|reply| Command::PairJoin {
            addr: host.addr,
            code,
            reply,
        })
        .await;

    // Assert
    assert!(result.is_err());
    assert!(trust.read().unwrap().pins.contains(&previous));
    assert!(!trust.read().unwrap().pins.contains(&host_identity.pin));
    assert!(
        requester
            .status()
            .await
            .peers
            .iter()
            .all(|peer| !peer.connected)
    );
    host.stop().await;
    requester.stop().await;
}

async fn relay(front: Connection, back: Connection) {
    async fn copy(mut recv: quinn::RecvStream, mut send: quinn::SendStream) {
        while let Ok(Ok(Some(chunk))) = timeout(
            limits::HANDSHAKE_TIMEOUT,
            recv.read_chunk(limits::CONTROL_FRAME_MAX, true),
        )
        .await
        {
            if !matches!(
                timeout(limits::HANDSHAKE_TIMEOUT, send.write_all(&chunk.bytes)).await,
                Ok(Ok(()))
            ) {
                break;
            }
        }
        let _ = send.finish();
    }
    let (front_send, front_recv) = front.accept_bi().await.unwrap();
    let (back_send, back_recv) = back.open_bi().await.unwrap();
    tokio::join!(copy(front_recv, back_send), copy(back_recv, front_send));
}

#[tokio::test]
async fn pair_join_terminating_relay_stores_nothing() {
    // Arrange
    let (host, requester) = (Peer::start(), Peer::start());
    let code = host.command(Command::PairHost).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let identity = identity::load_or_create(dir.path()).unwrap();
    let trust = Arc::new(RwLock::new(TrustSnapshot {
        pins: HashSet::default(),
        pairing_until: Some(Instant::now() + limits::PAIRING_CODE_DURATION),
    }));
    let mut endpoint = endpoint::bind_addr(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        tls::server_config(&identity, trust).unwrap(),
    )
    .unwrap();
    endpoint.set_default_client_config(tls::pair_client_config(&identity).unwrap());
    let addr = endpoint.local_addr().unwrap();
    let (connections, mut accepted) = mpsc::channel(1);
    let accept_task = tokio::spawn(endpoint::accept_loop(endpoint.clone(), connections));
    let proxy = tokio::spawn(async move {
        let (front, _permit, _) = accepted.recv().await.unwrap();
        let back = exchange::dial(&endpoint, host.addr).await.unwrap();
        relay(front, back).await;
        host
    });

    // Act
    let result = requester
        .command(|reply| Command::PairJoin { addr, code, reply })
        .await;

    // Assert
    assert!(matches!(
        result,
        Err(DaemonError::Pairing(PairingError::Refused))
    ));
    let host = timeout(test_peer::TEST_TIMEOUT, proxy)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(host.status().await.peers, []);
    assert_eq!(requester.status().await.peers, []);
    accept_task.abort();
    host.stop().await;
    requester.stop().await;
}
