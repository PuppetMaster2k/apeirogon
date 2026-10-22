// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::{Ipv4Addr, SocketAddr};

use super::*;
use crate::layout_store::LayoutStore;

#[tokio::test]
async fn receive_newer_peer_layout_replaces_and_persists() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let mut daemon = Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    let peer = MachineId::try_from(&[1; MachineId::LEN][..]).unwrap();
    let incoming = Layout::empty(peer)
        .edit(
            vec![Placement {
                id: peer,
                x: 0,
                y: 0,
                width: 100,
                height: 100,
            }],
            peer,
        )
        .unwrap();

    // Act
    daemon
        .receive(Envelope {
            body: Some(Body::Layout(incoming.to_proto())),
        })
        .unwrap();

    // Assert
    assert_eq!(daemon.layout.state, incoming);
    let restored = LayoutStore::load(&dir.path().join("layout.pb"), daemon.identity.machine_id);
    assert_eq!(restored.state, incoming);
    assert_eq!(daemon.layout_info().placed[0].name, "unknown");
}

#[tokio::test]
async fn receive_invalid_layout_ignores_without_link_error() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let mut daemon = Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    let before = daemon.layout.state.clone();
    let mut invalid = before.to_proto();
    invalid.version = u64::MAX;
    invalid.machines.push(v1::MachineRect {
        id: daemon.identity.machine_id.as_bytes().to_vec(),
        ..v1::MachineRect::default()
    });

    // Act
    let result = daemon.receive(Envelope {
        body: Some(Body::Layout(invalid)),
    });

    // Assert
    assert!(
        result.is_ok(),
        "the run loop closes a link only on receive errors"
    );
    assert_eq!(daemon.layout.state, before);
    assert!(!dir.path().join("layout.pb").exists());
}

#[tokio::test]
async fn receive_concurrent_winner_sets_notice_until_local_save() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let mut daemon = Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    daemon.set_layout(vec![]).unwrap();
    let mut winner = daemon.layout.state.to_proto();
    winner.by = vec![u8::MAX; MachineId::LEN];

    // Act
    daemon.receive_layout(&winner).unwrap();

    // Assert
    assert!(daemon.layout_info().lost_edit);
    daemon.set_layout(vec![]).unwrap();
    assert!(!daemon.layout_info().lost_edit);
    assert_eq!(daemon.layout.state.version(), 2);
}

#[test]
fn load_corrupt_or_invalid_layout_starts_empty() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let own = MachineId::try_from(&[1; MachineId::LEN][..]).unwrap();
    let path = dir.path().join("layout.pb");
    for bytes in [vec![255], vec![]] {
        std::fs::write(&path, bytes).unwrap();

        // Act
        let store = LayoutStore::load(&path, own);

        // Assert
        assert_eq!(store.state, Layout::empty(own));
    }
}
