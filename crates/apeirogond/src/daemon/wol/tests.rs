// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::SocketAddr;
use std::sync::mpsc;

use a7n_core::focus::Cursor;
use a7n_proto::{
    Pin,
    v1::{Envelope, envelope::Body},
};
use tempfile::TempDir;

use super::*;
use crate::pin_store::Entry;

fn offline() -> (Daemon, TempDir, MachineId, mpsc::Receiver<[u8; 6]>) {
    let dir = tempfile::tempdir().unwrap();
    let mut daemon = Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    let peer = MachineId::try_from(&[42; MachineId::LEN][..]).unwrap();
    daemon.pins.add(Entry {
        machine_id: peer,
        pin: Pin::try_from(&[42; 32][..]).unwrap(),
        name: "desktop".into(),
        spki_der: None,
        hw_addr: Some([2; 6]),
        screen_mm: None,
    });
    let (sender, receiver) = mpsc::channel();
    daemon.wol.sender = Box::new(move |mac| {
        sender.send(mac).unwrap();
        Ok(())
    });
    (daemon, dir, peer, receiver)
}

#[tokio::test]
async fn switch_machine_offline_records_wake_and_keeps_cursor_home() {
    // Arrange
    let (mut daemon, _dir, peer, packets) = offline();

    // Act
    daemon.switch_machine(peer).await.unwrap();

    // Assert
    assert_eq!(packets.try_recv().unwrap(), [2; 6]);
    assert_eq!(daemon.wol.pending.as_ref().unwrap().peer, peer);
    assert!(daemon.status().peers[0].waking);
    assert!(daemon.status().peers[0].can_wake);
    assert_eq!(daemon.input.focus.cursor(), Cursor::Home);
}

#[tokio::test]
async fn finish_wake_deadline_passed_clears_pending() {
    // Arrange
    let (mut daemon, _dir, peer, _packets) = offline();
    let now = Instant::now();
    daemon.auto_wake(peer, WakeAction::Switch, now).unwrap();

    // Act
    daemon.finish_wake(now + WAKE_TIMEOUT).await;

    // Assert
    assert!(daemon.wol.pending.is_none());
    assert!(!daemon.status().peers[0].waking);
    assert_eq!(daemon.input.focus.cursor(), Cursor::Home);
}

#[tokio::test]
async fn switch_machine_home_cancels_pending_take() {
    // Arrange
    let (mut daemon, _dir, peer, _packets) = offline();
    daemon.switch_machine(peer).await.unwrap();

    // Act
    daemon
        .switch_machine(daemon.identity.machine_id)
        .await
        .unwrap();

    // Assert
    assert!(daemon.wol.pending.is_none());
}

#[tokio::test]
async fn wake_peer_repeated_requests_throttles_per_peer() {
    // Arrange
    let (mut daemon, _dir, peer, packets) = offline();
    let now = Instant::now();
    daemon.wake_peer(peer, WakeAction::Only, now).unwrap();

    // Act
    daemon
        .wake_peer(peer, WakeAction::Only, now + WAKE_RESEND_INTERVAL / 2)
        .unwrap();
    daemon
        .wake_peer(peer, WakeAction::Only, now + WAKE_RESEND_INTERVAL)
        .unwrap();

    // Assert
    assert_eq!(packets.try_iter().count(), 2);
}

#[tokio::test]
async fn wake_peer_unknown_or_missing_mac_returns_actionable_error() {
    // Arrange
    let (mut daemon, _dir, peer, packets) = offline();
    let mut entry = daemon.pins.remove(&peer).unwrap();
    let unknown = daemon.wake_peer(peer, WakeAction::Only, Instant::now());
    entry.hw_addr = None;
    daemon.pins.add(entry);

    // Act
    let missing = daemon.wake_peer(peer, WakeAction::Only, Instant::now());

    // Assert
    assert!(matches!(unknown, Err(DaemonError::NotPaired)));
    assert!(
        missing
            .unwrap_err()
            .to_string()
            .contains("connect once while awake first")
    );
    assert!(packets.try_recv().is_err());
}

#[tokio::test]
async fn finish_wake_link_up_takes_once_with_original_landing() {
    for edge in [false, true] {
        // Arrange
        let (mut daemon, _dir, _, _packets) = offline();
        let remote_dir = tempfile::tempdir().unwrap();
        let remote = Daemon::load(
            remote_dir.path(),
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        )
        .unwrap();
        let peer = remote.identity.machine_id;
        daemon.pins.add(Entry {
            machine_id: peer,
            pin: remote.identity.pin,
            name: "remote".into(),
            spki_der: None,
            hw_addr: Some([2; 6]),
            screen_mm: None,
        });
        daemon.sync_trust().unwrap();
        remote
            .trust
            .write()
            .unwrap()
            .pins
            .insert(daemon.identity.pin);
        let landing = Landing {
            edge: a7n_proto::v1::Edge::Left.into(),
            offset: 0.25,
            ..Default::default()
        };
        let action = if edge {
            WakeAction::Edge(landing.clone())
        } else {
            WakeAction::Switch
        };
        daemon.auto_wake(peer, action, Instant::now()).unwrap();
        let addr = remote.endpoint.local_addr().unwrap();
        let remote_info = remote.info.clone();
        let accept = async {
            let connection = remote.endpoint.accept().await.unwrap().await.unwrap();
            crate::handshake::respond(connection, &remote_info, true)
                .await
                .unwrap()
        };
        let (link, mut remote_link) = tokio::join!(
            crate::endpoint::dial(&daemon.endpoint, addr, &daemon.info),
            accept
        );
        daemon.register(link.unwrap()).unwrap();
        let now = Instant::now();

        // Act
        daemon.finish_wake(now).await;
        daemon.finish_wake(now).await;

        // Assert
        assert!(daemon.wol.pending.is_none());
        let focus = tokio::time::timeout(crate::daemon::test_peer::TEST_TIMEOUT, async {
            loop {
                let Envelope { body } = crate::handshake::read_frame(
                    &mut remote_link.control_recv,
                    &mut remote_link.control_buf,
                    a7n_proto::FrameKind::Control,
                )
                .await
                .unwrap();
                if let Some(Body::Focus(focus)) = body {
                    break focus;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(focus.landing, edge.then_some(landing));
        assert_eq!(daemon.input.focus.cursor(), Cursor::Home);
        assert!(daemon.input.focus.next_deadline().is_some());
        assert!(matches!(
            daemon.wake_peer(peer, WakeAction::Only, now),
            Err(DaemonError::InvalidRequest("machine is already online"))
        ));
    }
}
