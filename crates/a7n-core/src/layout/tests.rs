// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn id(byte: u8) -> MachineId {
    MachineId::try_from(&[byte; MachineId::LEN][..]).unwrap()
}
fn rect(byte: u8, x: i32, y: i32) -> Placement {
    Placement {
        id: id(byte),
        x,
        y,
        width: 100,
        height: 100,
    }
}

#[test]
fn validate_invalid_rectangles_rejects_and_touching_accepts() {
    // Arrange
    let cases = [
        (
            vec![Placement {
                width: 0,
                ..rect(1, 0, 0)
            }],
            false,
        ),
        (
            vec![Placement {
                height: 0,
                ..rect(1, 0, 0)
            }],
            false,
        ),
        (vec![rect(1, 0, 0), rect(1, 100, 0)], false),
        (vec![rect(1, 0, 0), rect(2, 50, 50)], false),
        (vec![rect(1, i32::MAX, 0)], false),
        (vec![rect(1, 0, i32::MAX)], false),
        (vec![rect(1, 0, 0), rect(2, 100, 0)], true),
        (vec![rect(1, -100, -100), rect(2, 0, -100)], true),
        (vec![rect(1, 0, 0); limits::LAYOUT_MACHINES_MAX + 1], false),
    ];
    for (placements, valid) in cases {
        // Act
        let result = validate(&placements);

        // Assert
        assert_eq!(result.is_ok(), valid, "{placements:?}");
    }
}

#[test]
fn merge_version_and_author_orders_and_reports_lost_edit() {
    // Arrange
    let cases = [
        (2, 1, 1, 2, Merge::Unchanged),
        (
            1,
            1,
            2,
            2,
            Merge::Replaced {
                lost_own_edit: false,
            },
        ),
        (1, 2, 1, 1, Merge::Unchanged),
        (
            1,
            1,
            1,
            2,
            Merge::Replaced {
                lost_own_edit: true,
            },
        ),
        (
            1,
            2,
            1,
            3,
            Merge::Replaced {
                lost_own_edit: false,
            },
        ),
        (1, 1, 1, 1, Merge::Unchanged),
    ];
    for (version, author, next_version, next_author, expected) in cases {
        let mut layout = Layout {
            version,
            by: id(author),
            placements: vec![],
        };
        let incoming = Layout {
            version: next_version,
            by: id(next_author),
            placements: vec![],
        };
        let before = layout.clone();
        let wanted = if expected == Merge::Unchanged {
            before
        } else {
            incoming.clone()
        };

        // Act
        let outcome = layout.merge(incoming, id(1));

        // Assert
        assert_eq!(outcome, expected);
        assert_eq!(layout, wanted);
    }
}

#[test]
fn exit_touching_edges_maps_offset_and_gaps_are_walls() {
    // Arrange
    let layout = Layout::empty(id(1))
        .edit(vec![rect(1, 0, 0), rect(2, 100, 50)], id(1))
        .unwrap();
    let cases = [
        (0.75, Some(0.25)),
        (0.5, Some(0.0)),
        (0.25, None),
        (1.5, Some(0.5)),
        (-1.0, None),
        (f32::NAN, None),
    ];
    for (along, offset) in cases {
        // Act
        let landing = layout.exit(id(1), Edge::Right, along);

        // Assert
        assert_eq!(
            landing.map(|(target, landing)| (target, landing.edge, landing.offset)),
            offset.map(|offset| (id(2), Edge::Left as i32, offset))
        );
    }
}

#[test]
fn exit_all_directions_maps_to_opposite_edge_and_excludes_endpoint() {
    // Arrange
    let layout = Layout::empty(id(1))
        .edit(
            vec![
                rect(1, 0, 0),
                rect(2, 100, 0),
                rect(3, -100, 0),
                rect(4, 0, -100),
                rect(5, 0, 100),
            ],
            id(1),
        )
        .unwrap();
    for (edge, target, opposite) in [
        (Edge::Right, 2, Edge::Left),
        (Edge::Left, 3, Edge::Right),
        (Edge::Top, 4, Edge::Bottom),
        (Edge::Bottom, 5, Edge::Top),
    ] {
        // Act
        let landing = layout.exit(id(1), edge, 0.25).unwrap();

        // Assert
        assert_eq!(
            (landing.0, landing.1.edge, landing.1.offset),
            (id(target), opposite as i32, 0.25)
        );
        assert!(layout.exit(id(1), edge, 1.0).is_none());
    }
}

#[test]
fn shared_edges_partial_contact_lists_both_directions() {
    // Arrange
    let placements = [rect(1, 0, 0), rect(2, 100, 50), rect(3, 0, 200)];

    // Act
    let edges = shared_edges(&placements);

    // Assert
    assert_eq!(
        edges,
        vec![
            SharedEdge {
                from: id(1),
                to: id(2),
                edge: Edge::Right
            },
            SharedEdge {
                from: id(2),
                to: id(1),
                edge: Edge::Left
            }
        ]
    );
}

#[test]
fn edit_valid_placements_bumps_version_and_round_trips_wire() {
    // Arrange
    let layout = Layout::empty(id(1));
    let placements = vec![rect(1, 0, 0)];

    // Act
    let edited = layout.edit(placements.clone(), id(2)).unwrap();

    // Assert
    assert_eq!(edited.version(), 1);
    assert_eq!(edited.by(), id(2));
    assert_eq!(edited.placements(), placements);
    assert_eq!(Layout::try_from(&edited.to_proto()).unwrap(), edited);
}

#[test]
fn edit_exhausted_version_returns_error() {
    // Arrange
    let layout = Layout {
        version: u64::MAX,
        by: id(1),
        placements: vec![],
    };

    // Act
    let result = layout.edit(vec![], id(1));

    // Assert
    assert!(matches!(result, Err(LayoutError::VersionExhausted)));
}

#[test]
fn try_from_malformed_ids_and_overlapping_wire_rejects() {
    // Arrange
    let valid = Layout::empty(id(1))
        .edit(vec![rect(1, 0, 0), rect(2, 100, 0)], id(1))
        .unwrap()
        .to_proto();
    let mut bad_author = valid.clone();
    bad_author.by.clear();
    let mut bad_machine = valid.clone();
    bad_machine.machines[0].id.clear();
    let mut overlap = valid;
    overlap.machines[1].x = 50;
    for wire in [bad_author, bad_machine, overlap] {
        // Act
        let result = Layout::try_from(&wire);

        // Assert
        assert!(result.is_err());
    }
}
