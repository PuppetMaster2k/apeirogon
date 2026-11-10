// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::{Ipv4Addr, SocketAddr};

use a7n_core::{focus::Cursor, layout::Placement};
use a7n_proto::v1::{InputEvent, KeyEvent, input_event};
use tempfile::TempDir;

use super::super::Captured;
use super::*;
use crate::pin_store::Entry;

fn placed_daemon(target_y: i32) -> (Daemon, TempDir, [MachineId; 3]) {
    let dir = tempfile::tempdir().unwrap();
    let mut daemon = Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    let own = daemon.identity.machine_id;
    let target = MachineId::try_from(&[42; MachineId::LEN][..]).unwrap();
    let later = MachineId::try_from(&[43; MachineId::LEN][..]).unwrap();
    for id in [target, later] {
        daemon.pins.add(Entry {
            machine_id: id,
            pin: daemon.identity.pin,
            name: "peer".into(),
            spki_der: None,
            hw_addr: Some([2; 6]),
            screen_mm: None,
        });
    }
    let placements = [(own, 0, 0), (target, 100, target_y), (later, 200, 0)]
        .map(|(id, x, y)| Placement {
            id,
            x,
            y,
            width: 100,
            height: 100,
            blocked: 0,
        })
        .to_vec();
    daemon.layout.state = daemon.layout.state.edit(placements, own).unwrap();
    (daemon, dir, [own, target, later])
}

#[tokio::test]
async fn crossing_exit_unpaired_target_returns_no_crossing() {
    // Arrange
    for target_y in [0, 100] {
        let (mut daemon, _dir, [_, target, _]) = placed_daemon(target_y);
        daemon.unpair(target).unwrap();

        // Act
        let crossing =
            daemon.crossing_exit((0, 0, 100, 100), (99, 98), (100, 1), Edge::Right, 0.98);

        // Assert
        assert!(crossing.is_none());
    }
}

fn key_down(usage: u32) -> Captured {
    Captured::Input(InputEvent {
        event: Some(input_event::Event::Key(KeyEvent {
            usage,
            down: true,
            character: String::new(),
        })),
    })
}

#[tokio::test]
async fn match_hotkey_unpaired_slot_does_nothing_and_preserves_later_numbers() {
    // Arrange
    let (mut daemon, _dir, [own, target, later]) = placed_daemon(0);
    daemon.unpair(target).unwrap();
    daemon.wol.sender = Box::new(|_| panic!("an unpaired jump slot must not wake a later machine"));
    for usage in [0x0007_00e0, 0x0007_00e2, 0x0007_00e1] {
        daemon.match_hotkey(&key_down(usage)).await;
    }

    // Act
    let consumed = daemon.match_hotkey(&key_down(0x0007_001f)).await;

    // Assert
    assert!(consumed);
    assert_eq!(daemon.input.focus.cursor(), Cursor::Home);
    assert!(daemon.input.focus.next_deadline().is_none());
    assert!(daemon.wol.pending.is_none());
    assert_eq!(
        daemon.layout.state.nth(1, |id| daemon.is_layout_target(id)),
        Some(own)
    );
    assert_eq!(
        daemon.layout.state.nth(2, |id| daemon.is_layout_target(id)),
        None
    );
    assert_eq!(
        daemon.layout.state.nth(3, |id| daemon.is_layout_target(id)),
        Some(later)
    );
}

#[tokio::test]
async fn cross_edge_outside_left_point_reaches_paired_target() {
    // Arrange
    let (mut daemon, _dir, [own, target, _]) = placed_daemon(0);
    let mut placements = daemon.layout.state.placements().to_vec();
    placements[1].x = -100;
    daemon.layout.state = daemon.layout.state.edit(placements, own).unwrap();
    daemon.wol.sender = Box::new(|_| Ok(()));
    let bounds = Bounds {
        x: 0,
        y: 0,
        width: 100,
        height: 100,
    };
    let dx = -i32::try_from(a7n_proto::limits::DEFAULT_RESISTANCE_PX).unwrap();

    // Act
    daemon.cross_edge(bounds, -3, 50, dx, 0, false).await;

    // Assert
    let pending = daemon.wol.pending.as_ref().unwrap();
    assert_eq!(pending.peer, target);
    assert!(daemon.input.edge_no_target.is_none());
}
