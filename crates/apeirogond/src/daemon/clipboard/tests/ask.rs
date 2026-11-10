// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
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
        let result = daemon.accept_clipboard(peer, 10);

        // Assert
        assert!(matches!(result, Err(DaemonError::InvalidRequest(_))));
        assert!(daemon.clipboard.held.is_none());
        assert_eq!(daemon.clipboard.current, current);
        assert!(commands.try_recv().is_err());
    }
}

#[tokio::test]
async fn accept_clipboard_replaced_generation_returns_invalid_request_and_keeps_newer_held() {
    // Arrange
    let (mut daemon, _dir, _changes, mut commands) = daemon();
    let peer = daemon.identity.machine_id;
    daemon
        .config
        .set(&format!("peers.{peer}.clipboard"), "ask")
        .unwrap();
    daemon.receive_clipboard(peer, &wire(peer, 10, "held"));
    let seen = daemon.held_clipboard().unwrap();
    let newer = wire(peer, 11, "newer");
    daemon.receive_clipboard(peer, &newer);

    // Act
    let result = daemon.accept_clipboard(seen.machine, seen.generation);

    // Assert
    assert!(matches!(
        result,
        Err(DaemonError::InvalidRequest(
            "clipboard copy changed; review it again"
        ))
    ));
    assert_eq!(daemon.clipboard.held.as_ref().unwrap().wire, newer);
    assert_eq!(
        daemon.held_clipboard().unwrap().generation,
        newer.generation
    );
    assert!(daemon.clipboard.current.is_none());
    assert!(commands.try_recv().is_err());
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
async fn receive_clipboard_foreign_origin_is_ignored() {
    // Arrange
    let (mut daemon, _dir, _changes, mut commands) = daemon();
    let peer = daemon.identity.machine_id;
    let spoofed = MachineId::try_from([7; MachineId::LEN].as_slice()).unwrap();
    daemon.clipboard_changed(Some(change("local")));
    let previous = daemon.clipboard.current;

    // Act
    daemon.receive_clipboard(peer, &wire(spoofed, 10, "spoofed"));

    // Assert
    assert!(commands.try_recv().is_err());
    assert_eq!(daemon.clipboard.current, previous);
}
