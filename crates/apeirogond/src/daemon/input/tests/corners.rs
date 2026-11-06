// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[tokio::test]
async fn cross_edge_diagonal_push_uses_corner_unless_edge_exists_or_segment_is_blocked() {
    // Arrange
    for (dy, edge_neighbour, blocked, expected) in [
        (1, false, 0, Some(2)),
        (0, false, 0, None),
        (1, true, 0, Some(3)),
        (1, false, 1 << 8, None),
        (1, true, 1 << 8, None),
        (1, false, 1 << 6, Some(2)),
    ] {
        let (mut daemon, _dir, _input) = daemon(true);
        let own = daemon.identity.machine_id;
        let mut placements = vec![Placement {
            id: own,
            x: 0,
            y: 0,
            width: 100,
            height: 100,
            blocked,
        }];
        for (byte, y) in [(2, 100), (3, 0)] {
            if byte == 3 && !edge_neighbour {
                continue;
            }
            let id = MachineId::try_from(&[byte; MachineId::LEN][..]).unwrap();
            daemon.pins.add(Entry {
                machine_id: id,
                pin: daemon.identity.pin,
                name: format!("peer-{byte}"),
                spki_der: None,
                hw_addr: Some([byte; 6]),
                screen_mm: None,
            });
            placements.push(Placement {
                id,
                x: 100,
                y,
                width: 100,
                height: 100,
                blocked: u16::MAX,
            });
        }
        daemon.layout.state = daemon.layout.state.edit(placements, own).unwrap();
        daemon.wol.sender = Box::new(|_| Ok(()));
        let bounds = daemon.input.capture.as_ref().unwrap().bounds;
        let dx = i32::try_from(limits::DEFAULT_RESISTANCE_PX).unwrap();

        // Act
        daemon.cross_edge(bounds, 99, 98, dx, dy, false).await;

        // Assert
        assert_eq!(
            daemon.wol.pending.as_ref().map(|pending| pending.peer),
            expected.map(|byte| MachineId::try_from(&[byte; MachineId::LEN][..]).unwrap())
        );
    }
}
