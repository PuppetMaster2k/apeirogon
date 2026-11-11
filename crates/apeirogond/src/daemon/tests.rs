// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::Ipv4Addr;

use tokio::time::timeout;

use super::test_peer::{Peer, paired};
use super::*;

#[tokio::test]
async fn joined_previously_revoked_host_keeps_pin_after_reload() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let host_dir = tempfile::tempdir().unwrap();
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let mut joiner = Daemon::load(dir.path(), addr).unwrap();
    let host = identity::load_or_create(host_dir.path()).unwrap();
    let entry = Entry {
        machine_id: host.machine_id,
        pin: host.pin,
        name: "host".into(),
        spki_der: Some(identity::spki_of(&host.cert_der).unwrap()),
        hw_addr: None,
        screen_mm: None,
    };
    joiner.pins.add(entry.clone());
    let revoked_at = 1;
    joiner
        .desk
        .revoke(
            &joiner.identity,
            &mut joiner.pins,
            host.machine_id,
            revoked_at,
        )
        .unwrap();
    let own_id = joiner.identity.machine_id;
    let (reply, _receiver) = oneshot::channel();

    // Act
    joiner.joined(Ok(entry.clone()), addr, reply);
    // Abort before yielding so the post-pair connection never runs.
    joiner.tasks.abort_all();
    drop(joiner);
    let pins_path = dir.path().join("pins.toml");
    let mut pins = PinStore::load(&pins_path).unwrap();
    let desk = DeskStore::load(&dir.path().join("desk.pb"), own_id, &mut pins).unwrap();

    // Assert
    assert_eq!(pins.get(&host.machine_id), Some(&entry));
    assert_eq!(
        PinStore::load(&pins_path).unwrap().get(&host.machine_id),
        Some(&entry)
    );
    assert!(
        desk.to_desk().members.iter().any(|member| {
            member.id == host.machine_id.as_bytes() && member.added_at > revoked_at
        })
    );
}

#[tokio::test]
async fn load_existing_self_pin_removes_it_from_memory_and_disk() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let peer_dir = tempfile::tempdir().unwrap();
    let own = identity::load_or_create(dir.path()).unwrap();
    let peer = identity::load_or_create(peer_dir.path()).unwrap();
    let path = dir.path().join("pins.toml");
    let mut pins = PinStore::load(&path).unwrap();
    for (identity, name) in [(&own, "self"), (&peer, "peer")] {
        pins.add(Entry {
            machine_id: identity.machine_id,
            pin: identity.pin,
            name: name.into(),
            spki_der: Some(identity::spki_of(&identity.cert_der).unwrap()),
            hw_addr: None,
            screen_mm: None,
        });
    }
    pins.save().unwrap();
    let peer_entry = pins.get(&peer.machine_id).unwrap().clone();
    let own_entry = pins.get(&own.machine_id).unwrap().clone();
    let mut peer_pins = PinStore::load(&peer_dir.path().join("pins.toml")).unwrap();
    let mut desk =
        DeskStore::load(&dir.path().join("desk.pb"), peer.machine_id, &mut peer_pins).unwrap();
    desk.admit(&peer, &mut peer_pins, &own_entry, 1).unwrap();

    // Act
    let daemon = Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();

    // Assert
    assert!(daemon.pins.get(&own.machine_id).is_none());
    assert_eq!(daemon.pins.get(&peer.machine_id), Some(&peer_entry));
    let reloaded = PinStore::load(&path).unwrap();
    assert!(reloaded.get(&own.machine_id).is_none());
    assert_eq!(reloaded.get(&peer.machine_id), Some(&peer_entry));
}

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
