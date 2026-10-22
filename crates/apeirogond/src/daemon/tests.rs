// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::Ipv4Addr;

use a7n_proto::v1::Capability;
use tokio::time::timeout;

use super::test_peer::{Peer, linked};
use super::*;

#[tokio::test]
async fn pair_join_two_daemons_stores_pins_and_links_both_sides() {
    // Arrange
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let mut a_daemon = Daemon::load(a_dir.path(), addr).unwrap();
    let mut b_daemon = Daemon::load(b_dir.path(), addr).unwrap();
    a_daemon.info.capabilities = vec![
        Capability::ClipboardText as i32,
        Capability::Gestures as i32,
    ];
    b_daemon.info.capabilities = vec![Capability::ClipboardText as i32];
    let (a, b) = (Peer::run(a_daemon, a_dir), Peer::run(b_daemon, b_dir));

    // Act
    a.pair(&b).await;

    // Assert
    assert!(a.status().await.paired.contains(&b.machine));
    assert!(b.status().await.paired.contains(&a.machine));
    assert_eq!(
        a.status().await.linked[0].capabilities,
        vec![Capability::ClipboardText]
    );
    assert_eq!(
        b.status().await.linked[0].capabilities,
        vec![Capability::ClipboardText]
    );
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
    b.wait(|status| status.paired.contains(&c.machine)).await;
    c.wait(|status| status.paired.contains(&b.machine)).await;
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
    b.wait(|status| status.paired.contains(&c.machine)).await;
    c.wait(|status| status.paired.contains(&b.machine)).await;
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
    assert!(!a_status.paired.contains(&b.machine));
    assert!(!linked(&a_status, b.machine));
    c.wait(|status| !status.paired.contains(&b.machine) && !linked(status, b.machine))
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
    assert!(a.status().await.linked.is_empty());
    assert_eq!(a.status().await.paired, []);
    a.stop().await;
}

#[tokio::test]
async fn connect_simultaneous_dials_keeps_larger_initiator_and_live_registry() {
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
        let expected = if local.machine > remote.machine {
            crate::link::Role::Initiator
        } else {
            crate::link::Role::Responder
        };
        let status = local
            .wait(|status| {
                status
                    .linked
                    .iter()
                    .any(|peer| peer.machine == remote.machine && peer.role == expected)
            })
            .await;
        assert_eq!(status.linked.len(), 1);
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
    assert_eq!(a.status().await.paired, []);
    assert_eq!(b.status().await.paired, []);
    a.stop().await;
    b.stop().await;
}
