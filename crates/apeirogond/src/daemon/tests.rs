// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::Ipv4Addr;

use tokio::time::timeout;

use super::test_peer::{Peer, linked, paired};
use super::*;

#[tokio::test]
async fn pair_join_two_daemons_stores_pins_and_links_both_sides() {
    // Arrange
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let a_daemon = Daemon::load(a_dir.path(), addr).unwrap();
    let b_daemon = Daemon::load(b_dir.path(), addr).unwrap();
    let (a, b) = (Peer::run(a_daemon, a_dir), Peer::run(b_daemon, b_dir));

    // Act
    let entry = a.pair(&b).await;

    // Assert
    assert_eq!(entry.machine_id, a.machine);
    assert_eq!(entry.name, a.status().await.name);
    assert!(paired(&a.status().await, b.machine));
    assert!(paired(&b.status().await, a.machine));
    let a_pins = PinStore::load(&a.dir.path().join("pins.toml")).unwrap();
    let b_pins = PinStore::load(&b.dir.path().join("pins.toml")).unwrap();
    assert!(a_pins.get(&b.machine).unwrap().spki_der.is_some());
    assert!(b_pins.get(&a.machine).unwrap().spki_der.is_some());
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn pair_host_three_daemons_propagates_membership_and_trust() {
    // Arrange
    let (a, b, c) = (Peer::start(), Peer::start(), Peer::start());
    a.pair(&b).await;

    // Act
    a.pair(&c).await;

    // Assert
    b.wait(|status| paired(status, c.machine)).await;
    c.wait(|status| paired(status, b.machine)).await;
    // Membership received through A is sufficient for a pinned B-C handshake.
    b.command(|reply| Command::Connect {
        addr: c.addr,
        reply,
    })
    .await
    .unwrap();
    c.wait(|status| linked(status, b.machine)).await;
    a.stop().await;
    b.stop().await;
    c.stop().await;
}

#[tokio::test]
async fn unpair_linked_member_removes_remote_pin_and_closes_links() {
    // Arrange
    let (a, b, c) = (Peer::start(), Peer::start(), Peer::start());
    a.pair(&b).await;
    a.pair(&c).await;
    b.wait(|status| paired(status, c.machine)).await;
    c.wait(|status| paired(status, b.machine)).await;
    c.command(|reply| Command::Connect {
        addr: b.addr,
        reply,
    })
    .await
    .unwrap();
    b.wait(|status| linked(status, c.machine)).await;

    // Act
    a.command(|reply| Command::Unpair {
        machine: b.machine,
        reply,
    })
    .await
    .unwrap();

    // Assert
    let a_status = a.status().await;
    assert!(!paired(&a_status, b.machine));
    assert!(!linked(&a_status, b.machine));
    c.wait(|status| !paired(status, b.machine) && !linked(status, b.machine))
        .await;
    b.wait(|status| !linked(status, a.machine)).await;
    let pins = PinStore::load(&c.dir.path().join("pins.toml")).unwrap();
    assert!(pins.get(&b.machine).is_none());
    a.stop().await;
    b.stop().await;
    c.stop().await;
}

#[tokio::test]
async fn accept_unknown_control_pin_during_pairing_window_closes_before_hello() {
    // Arrange
    let a = Peer::start();
    a.command(Command::PairHost).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let identity = identity::load_or_create(dir.path()).unwrap();
    let host_cert = identity::load_or_create(a.dir.path()).unwrap();
    let trust = Arc::new(RwLock::new(TrustSnapshot {
        pins: [host_cert.pin].into_iter().collect(),
        pairing_until: None,
    }));
    let mut endpoint = endpoint::bind_addr(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        tls::server_config(&identity, Arc::clone(&trust)).unwrap(),
    )
    .unwrap();
    endpoint.set_default_client_config(tls::client_config(&identity, trust).unwrap());

    // Act
    let connection = endpoint
        .connect(a.addr, tls::TLS_SERVER_NAME)
        .unwrap()
        .await
        .unwrap();
    let closed = timeout(limits::HANDSHAKE_TIMEOUT, connection.closed())
        .await
        .unwrap();

    // Assert
    assert!(matches!(
        closed,
        quinn::ConnectionError::ApplicationClosed(_)
    ));
    assert!(a.status().await.peers.iter().all(|peer| !peer.connected));
    assert_eq!(a.status().await.peers, []);
    a.stop().await;
}

#[tokio::test]
async fn connect_simultaneous_dials_keeps_live_registry() {
    // Arrange
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let mut a = Daemon::load(a_dir.path(), addr).unwrap();
    let mut b = Daemon::load(b_dir.path(), addr).unwrap();
    let a_identity = (a.identity.machine_id, a.identity.pin);
    let b_identity = (b.identity.machine_id, b.identity.pin);
    for (local, (machine_id, pin)) in [(&mut a, b_identity), (&mut b, a_identity)] {
        local.pins.add(crate::pin_store::Entry {
            machine_id,
            pin,
            name: "peer".into(),
            spki_der: None,
        });
        local.pins.save().unwrap();
        local.sync_trust().unwrap();
    }
    let (a, b) = (Peer::run(a, a_dir), Peer::run(b, b_dir));

    // Act
    let (a_result, b_result) = tokio::join!(
        a.command(|reply| Command::Connect {
            addr: b.addr,
            reply
        }),
        b.command(|reply| Command::Connect {
            addr: a.addr,
            reply
        }),
    );

    // Assert
    a_result.unwrap();
    b_result.unwrap();
    for (local, remote) in [(&a, &b), (&b, &a)] {
        let status = local.wait(|status| linked(status, remote.machine)).await;
        assert_eq!(status.peers.iter().filter(|peer| peer.connected).count(), 1);
    }
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn pair_join_wrong_code_closes_listener_window_without_storing_pins() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let daemon = Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    let trust = Arc::clone(&daemon.trust);
    let (a, b) = (Peer::run(daemon, dir), Peer::start());
    let code = a.command(Command::PairHost).await.unwrap();
    let wrong = if code.to_string() == "000000" {
        "000001"
    } else {
        "000000"
    };

    // Act
    let result = b
        .command(|reply| Command::PairJoin {
            addr: a.addr,
            code: PairingCode::parse(wrong).unwrap(),
            reply,
        })
        .await;

    // Assert
    assert!(result.is_err());
    assert!(!trust.read().unwrap().pairing_open());
    assert_eq!(a.status().await.peers, []);
    assert_eq!(b.status().await.peers, []);
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn status_ipc_round_trip_returns_own_machine() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let mut daemon = Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    daemon.info.name = "own machine".into();
    let offline = MachineId::try_from(&[1; MachineId::LEN][..]).unwrap();
    daemon.pins.add(Entry {
        machine_id: offline,
        pin: daemon.identity.pin,
        name: "offline peer".into(),
        spki_der: None,
    });
    let peer = Peer::run(daemon, dir);
    let socket_dir = tempfile::tempdir().unwrap();
    let location = a7n_ipc::SocketLocation::new(socket_dir.path().join("private/control.sock"));
    let server = a7n_ipc::IpcServer::bind_at(&location).await.unwrap();
    let server_task = tokio::spawn(crate::ipc_server::serve(
        server,
        peer.sender.as_ref().unwrap().clone(),
    ));
    let mut client = a7n_ipc::IpcClient::connect_at(&location).await.unwrap();

    // Act
    let response = client.request(&a7n_ipc::Request::Status).await.unwrap();

    // Assert
    let a7n_ipc::Response::Status(info) = response else {
        panic!("expected status")
    };
    assert_eq!(
        info,
        a7n_ipc::StatusInfo {
            machine: peer.machine,
            name: "own machine".into(),
            nearby: vec![],
            peers: vec![a7n_ipc::PeerInfo {
                id: offline,
                name: "offline peer".into(),
                connected: false,
                rtt_ms: None,
            }],
        }
    );
    drop(client);
    server_task.abort();
    let _ = server_task.await;
    peer.stop().await;
}

#[tokio::test]
async fn unpair_unknown_machine_returns_not_paired_without_revocation() {
    // Arrange
    let peer = Peer::start();
    let unknown = MachineId::try_from(&[0; MachineId::LEN][..]).unwrap();
    let socket_dir = tempfile::tempdir().unwrap();
    let location = a7n_ipc::SocketLocation::new(socket_dir.path().join("private/control.sock"));
    let server = a7n_ipc::IpcServer::bind_at(&location).await.unwrap();
    let server_task = tokio::spawn(crate::ipc_server::serve(
        server,
        peer.sender.as_ref().unwrap().clone(),
    ));
    let mut client = a7n_ipc::IpcClient::connect_at(&location).await.unwrap();

    for machine in [unknown, peer.machine] {
        // Act
        let response = client
            .request(&a7n_ipc::Request::Unpair { machine })
            .await
            .unwrap();

        // Assert
        assert!(matches!(
            response,
            a7n_ipc::Response::Error {
                kind: a7n_ipc::ErrorKind::NotPaired,
                ..
            }
        ));
        let mut pins = PinStore::load(&peer.dir.path().join("pins.toml")).unwrap();
        let desk =
            DeskStore::load(&peer.dir.path().join("desk.pb"), peer.machine, &mut pins).unwrap();
        assert_eq!(desk.to_desk().revocations, []);
        assert!(!peer.dir.path().join("desk.pb").exists());
    }
    drop(client);
    server_task.abort();
    let _ = server_task.await;
    peer.stop().await;
}
