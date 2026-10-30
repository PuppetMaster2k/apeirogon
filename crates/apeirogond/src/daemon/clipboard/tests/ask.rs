// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::command::Command;
use crate::daemon::DaemonError;

#[tokio::test]
async fn receive_clipboard_ask_policy_holds_without_applying() {
    // Arrange
    let (mut daemon, _dir, _changes, mut commands) = daemon();
    let peer = daemon.identity.machine_id;
    daemon
        .config
        .set(&format!("peers.{peer}.clipboard"), "ask")
        .unwrap();
    daemon.clipboard_changed(Some(change("local")));
    let previous = daemon.clipboard.current;
    let incoming = wire(peer, 10, "private contents");

    // Act
    daemon.receive_clipboard(peer, &incoming);

    // Assert
    assert!(commands.try_recv().is_err());
    assert_eq!(daemon.clipboard.current, previous);
    assert_eq!(daemon.clipboard.held.as_ref().unwrap().wire, incoming);
    let info = daemon.clipboard_status();
    assert_eq!(info.held, daemon.status().held_clipboard);
    let held = info.held.unwrap();
    assert_eq!(held.machine, peer);
    assert_eq!(held.items, 1);
    assert_eq!(held.size, 16);
    assert_eq!(held.kinds, [a7n_ipc::ClipboardKind::Text]);
}

#[tokio::test]
async fn accept_clipboard_held_newest_applies_copy() {
    // Arrange
    let (mut a, a_dir, changes, _a_commands) = daemon();
    let (mut b, b_dir, _b_changes, mut commands) = daemon();
    let peer = a.identity.machine_id;
    b.config
        .set(&format!("peers.{peer}.clipboard"), "ask")
        .unwrap();
    a.clipboard_changed(Some(change("first")));
    let (a, b) = (Peer::run(a, a_dir), Peer::run(b, b_dir));
    a.pair(&b).await;
    b.wait(|s| s.held_clipboard.is_some()).await;
    changes.send(change("newest")).await.unwrap();
    b.wait(|s| s.held_clipboard.as_ref().is_some_and(|held| held.size == 6))
        .await;
    assert!(commands.try_recv().is_err());

    // Act
    let result = b.command(Command::ClipboardAccept).await;

    // Assert
    assert!(result.is_ok());
    let ClipboardCommand::Set {
        generation, items, ..
    } = commands.try_recv().unwrap();
    assert_eq!(generation.generation, 2);
    assert_eq!(items, change("newest").items);
    assert!(b.status().await.held_clipboard.is_none());
    assert!(commands.try_recv().is_err());
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn accept_clipboard_newer_generation_arrived_is_rejected() {
    for local in [false, true] {
        // Arrange
        let (mut daemon, _dir, _changes, mut commands) = daemon();
        let peer = daemon.identity.machine_id;
        daemon
            .config
            .set(&format!("peers.{peer}.clipboard"), "ask")
            .unwrap();
        daemon.receive_clipboard(peer, &wire(peer, 10, "held"));
        if local {
            daemon.clipboard_changed(Some(change("local")));
        } else {
            let other = MachineId::try_from([42; MachineId::LEN].as_slice()).unwrap();
            daemon.receive_clipboard(other, &wire(other, 11, "newer"));
            commands.try_recv().unwrap();
        }
        let current = daemon.clipboard.current;

        // Act
        let result = daemon.accept_clipboard();

        // Assert
        assert!(matches!(result, Err(DaemonError::InvalidRequest(_))));
        assert!(daemon.clipboard.held.is_none());
        assert_eq!(daemon.clipboard.current, current);
        assert!(commands.try_recv().is_err());
    }
}

#[tokio::test]
async fn reject_clipboard_held_drops_copy() {
    // Arrange
    let (mut daemon, _dir, _changes, mut commands) = daemon();
    let peer = daemon.identity.machine_id;
    daemon
        .config
        .set(&format!("peers.{peer}.clipboard"), "ask")
        .unwrap();
    daemon.receive_clipboard(peer, &wire(peer, 10, "held"));

    // Act
    let result = daemon.reject_clipboard();

    // Assert
    assert!(result.is_ok());
    assert!(daemon.clipboard.held.is_none());
    assert!(daemon.clipboard.current.is_none());
    assert!(commands.try_recv().is_err());
    assert!(matches!(
        daemon.reject_clipboard(),
        Err(DaemonError::InvalidRequest(_))
    ));
}

#[tokio::test]
async fn expire_held_clipboard_past_deadline_drops_copy() {
    // Arrange
    let (mut daemon, _dir, _changes, mut commands) = daemon();
    let peer = daemon.identity.machine_id;
    daemon
        .config
        .set(&format!("peers.{peer}.clipboard"), "ask")
        .unwrap();
    daemon.receive_clipboard(peer, &wire(peer, 10, "held"));
    let deadline = daemon.clipboard.held.as_ref().unwrap().deadline;
    daemon
        .clipboard
        .expire_held_clipboard(deadline.checked_sub(Duration::from_nanos(1)).unwrap());
    assert!(daemon.clipboard.held.is_some());

    // Act
    daemon.clipboard.expire_held_clipboard(deadline);

    // Assert
    assert!(daemon.clipboard.held.is_none());
    assert!(daemon.clipboard.current.is_none());
    assert!(commands.try_recv().is_err());
}

#[tokio::test]
async fn link_down_held_peer_drops_copy() {
    // Arrange
    let (mut a, a_dir, _changes, _a_commands) = daemon();
    let (mut b, b_dir, _b_changes, mut commands) = daemon();
    let peer = a.identity.machine_id;
    b.config
        .set(&format!("peers.{peer}.clipboard"), "ask")
        .unwrap();
    a.clipboard_changed(Some(change("held")));
    let (a, b) = (Peer::run(a, a_dir), Peer::run(b, b_dir));
    a.pair(&b).await;
    b.wait(|s| s.held_clipboard.is_some()).await;

    // Act
    a.stop().await;

    // Assert
    b.wait(|s| !super::super::super::test_peer::linked(s, peer))
        .await;
    assert!(b.status().await.held_clipboard.is_none());
    assert!(commands.try_recv().is_err());
    b.stop().await;
}

#[tokio::test]
async fn receive_clipboard_ask_stale_or_invalid_keeps_newest_held() {
    // Arrange
    let (mut daemon, _dir, _changes, mut commands) = daemon();
    let peer = daemon.identity.machine_id;
    daemon
        .config
        .set(&format!("peers.{peer}.clipboard"), "ask")
        .unwrap();
    let newest = wire(peer, 10, "newest");
    daemon.receive_clipboard(peer, &newest);
    let mut invalid = wire(peer, 11, "invalid");
    invalid.items[0].sha256[0] ^= 1;

    // Act
    daemon.receive_clipboard(peer, &wire(peer, 9, "older"));
    daemon.receive_clipboard(peer, &invalid);

    // Assert
    assert_eq!(daemon.clipboard.held.as_ref().unwrap().wire, newest);
    assert!(commands.try_recv().is_err());
    assert!(daemon.clipboard.current.is_none());
}

#[tokio::test]
async fn accept_clipboard_link_gone_returns_not_connected() {
    // Arrange
    let (mut daemon, _dir, _changes, mut commands) = daemon();
    let peer = daemon.identity.machine_id;
    daemon
        .config
        .set(&format!("peers.{peer}.clipboard"), "ask")
        .unwrap();
    daemon.receive_clipboard(peer, &wire(peer, 10, "held"));

    // Act
    let result = daemon.accept_clipboard();

    // Assert
    assert!(matches!(result, Err(DaemonError::NotConnected)));
    assert!(daemon.clipboard.held.is_none());
    assert!(commands.try_recv().is_err());
}
