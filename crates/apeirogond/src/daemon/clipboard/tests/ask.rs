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
