// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::{Ipv4Addr, SocketAddr};

use a7n_core::layout::Layout;

use super::*;
use crate::layout_store::LayoutStore;

#[tokio::test]
async fn receive_sync_from_layout_burst_persists_first_and_latest_after_tick() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let mut daemon = Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    let peer = MachineId::try_from(&[1; MachineId::LEN][..]).unwrap();
    let now = Instant::now();
    let mut layout = Layout::empty(peer);
    let messages: Vec<_> = (0..3)
        .map(|_| {
            layout = layout.edit(vec![], peer).unwrap();
            Envelope {
                body: Some(Body::Layout(layout.to_proto())),
            }
        })
        .collect();
    let path = dir.path().join("layout.pb");

    // Act
    daemon
        .receive_sync_from(peer, messages[0].clone(), now)
        .await
        .unwrap();
    let first = std::fs::read(&path).unwrap();
    let first_version = daemon.layout.state.version();
    for message in &messages[1..] {
        daemon
            .receive_sync_from(peer, message.clone(), now + SYNC_MIN_INTERVAL / 2)
            .await
            .unwrap();
    }
    daemon.flush_sync(now + SYNC_MIN_INTERVAL / 2).await;
    let before_tick = std::fs::read(&path).unwrap();
    daemon.flush_sync(now + SYNC_MIN_INTERVAL).await;
    let restored = LayoutStore::load(&path, daemon.identity.machine_id);

    // Assert
    assert_eq!(first_version, 1);
    assert_eq!(first, before_tick);
    assert_eq!(daemon.layout.state, layout);
    assert_eq!(restored.state, layout);
    assert!(
        daemon
            .sync
            .incoming
            .values()
            .all(|pending| pending.message.is_none())
    );
}
