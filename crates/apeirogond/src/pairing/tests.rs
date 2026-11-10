// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::time::Duration;

use quinn::Connection;
use tokio::sync::mpsc;
use tokio::time::timeout;

use super::test_peer::Peer;
use super::*;
use crate::pairing::{self as exchange, PairingError};

const RESPONSIVE_TIMEOUT: Duration = Duration::from_secs(1);

#[tokio::test]
async fn store_peer_own_machine_refuses_without_storing_pin_or_trust() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let mut daemon = Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    let own_id = daemon.identity.machine_id;
    let entry = Entry {
        machine_id: own_id,
        pin: daemon.identity.pin,
        name: "self".into(),
        spki_der: None,
        hw_addr: None,
        screen_mm: None,
    };

    // Act
    let result = daemon.store_peer(entry);

    // Assert
    assert!(matches!(
        result,
        Err(DaemonError::Pairing(PairingError::Refused))
    ));
    assert!(daemon.pins.get(&own_id).is_none());
    assert!(!dir.path().join("pins.toml").exists());
    assert!(daemon.trust.read().unwrap().pins.is_empty());
}

#[tokio::test]
async fn pair_join_expired_code_stores_nothing() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let mut daemon = Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    let code = daemon.open_pairing().unwrap();
    daemon.pairing.host = Some(HostPairing::new(
        code.clone(),
        None,
        daemon.identity.pin,
        Instant::now()
            .checked_sub(limits::PAIRING_CODE_DURATION)
            .unwrap(),
    ));
    let (host, requester) = (Peer::run(daemon, dir), Peer::start());

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
    assert_eq!(host.status().await.peers, []);
    assert_eq!(requester.status().await.peers, []);
    host.stop().await;
    requester.stop().await;
}

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

#[tokio::test]
async fn pair_host_stalled_requester_keeps_commands_responsive_and_refuses_second_attempt() {
    // Arrange
    let (host, second) = (Peer::start(), Peer::start());
    let code = host.command(Command::PairHost).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut attacker =
        Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    attacker
        .endpoint
        .set_default_client_config(tls::pair_client_config(&attacker.identity).unwrap());
    let connection = exchange::dial(&attacker.endpoint, host.addr).await.unwrap();
    // Sending Start reaches the exchange but withholding Confirm stalls it.
    let requester = a7n_core::pairing::RequesterPairing::new(
        code.clone(),
        None,
        attacker.identity.pin,
        attacker.info.clone(),
        &mut OsRng,
    );
    let (mut send, mut recv) = connection.open_bi().await.unwrap();
    exchange::write_message(
        &mut send,
        a7n_proto::v1::pairing_message::Body::Start(requester.start()),
    )
    .await
    .unwrap();
    exchange::read_message(&mut recv, &mut Vec::new())
        .await
        .unwrap();

    // Act
    let (status, reopen, second_result) = timeout(RESPONSIVE_TIMEOUT, async {
        let status = host.status().await;
        let reopen = host.command(Command::PairHost).await;
        let second_result = second
            .command(|reply| Command::PairJoin {
                addr: host.addr,
                code,
                reply,
            })
            .await;
        (status, reopen, second_result)
    })
    .await
    .expect("stalled host exchange blocked the owner");

    // Assert
    assert_eq!(status.peers, []);
    assert!(matches!(reopen, Err(DaemonError::PairingBusy)));
    assert!(second_result.is_err());
    connection.close(0u32.into(), b"");
    host.stop().await;
    second.stop().await;
}
