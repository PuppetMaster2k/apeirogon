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
        blocked: 0,
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
    let mut overlap = valid.clone();
    overlap.machines[1].x = 50;
    let mut huge_version = valid;
    huge_version.version = limits::COUNTER_MAX + 1;
    for wire in [bad_author, bad_machine, overlap, huge_version] {
        // Act
        let result = Layout::try_from(&wire);

        // Assert
        assert!(result.is_err());
    }
}

#[test]
fn nth_unsorted_placements_returns_one_based_canvas_order() {
    // Arrange
    let layout = Layout::empty(id(1))
        .edit(
            vec![
                rect(1, 0, 0),
                rect(2, 100, -100),
                rect(3, -100, -100),
                rect(4, -100, 0),
            ],
            id(1),
        )
        .unwrap();

    // Act
    let order: Vec<_> = (0..=5).map(|n| layout.nth(n)).collect();

    // Assert
    assert_eq!(
        order,
        vec![
            None,
            Some(id(3)),
            Some(id(2)),
            Some(id(4)),
            Some(id(1)),
            None
        ]
    );
}

#[test]
fn exit_blocked_thirds_only_prevent_source_segment_crossings() {
    // Arrange
    for (edge, x, y, index) in [
        (Edge::Left, -100, 0, 0),
        (Edge::Top, 0, -100, 1),
        (Edge::Right, 100, 0, 2),
        (Edge::Bottom, 0, 100, 3),
    ] {
        for third in 0..3 {
            let source = Placement {
                blocked: 1 << (index * 3 + third),
                ..rect(1, 0, 0)
            };
            let target = Placement {
                blocked: u16::MAX,
                ..rect(2, x, y)
            };
            let layout = Layout::empty(id(1))
                .edit(vec![source, target], id(1))
                .unwrap();
            for (segment, along) in [0.1, 0.5, 0.9].into_iter().enumerate() {
                // Act
                let exit = layout.exit(id(1), edge, along);

                // Assert
                assert_eq!(exit.is_none(), segment == third);
            }
        }
    }
}

#[test]
fn exit_corner_diagonal_neighbours_land_on_opposite_edge_at_shared_corner() {
    // Arrange
    let layout = Layout::empty(id(1))
        .edit(
            vec![
                rect(1, 0, 0),
                rect(2, -100, -100),
                rect(3, 100, -100),
                rect(4, -100, 100),
                rect(5, 100, 100),
            ],
            id(1),
        )
        .unwrap();
    for (corner, edge, target, opposite, offset) in [
        (Corner::TopLeft, Edge::Left, 2, Edge::Right, 1.0),
        (Corner::TopLeft, Edge::Top, 2, Edge::Bottom, 1.0),
        (Corner::TopRight, Edge::Right, 3, Edge::Left, 1.0),
        (Corner::TopRight, Edge::Top, 3, Edge::Bottom, 0.0),
        (Corner::BottomLeft, Edge::Left, 4, Edge::Right, 0.0),
        (Corner::BottomLeft, Edge::Bottom, 4, Edge::Top, 1.0),
        (Corner::BottomRight, Edge::Right, 5, Edge::Left, 0.0),
        (Corner::BottomRight, Edge::Bottom, 5, Edge::Top, 0.0),
    ] {
        // Act
        let (target_id, landing) = layout.exit_corner(id(1), edge, corner).unwrap();

        // Assert
        assert_eq!(
            (target_id, landing.edge, landing.offset),
            (id(target), opposite as i32, offset)
        );
    }
}

#[test]
fn try_from_unknown_and_absent_blocked_bits_masks_to_known_segments() {
    use a7n_proto::{FrameKind, decode_frame, encode_frame};

    // Arrange
    for (blocked, expected) in [(u32::MAX, BLOCKED_MASK), (0, 0)] {
        let mut wire = Layout::empty(id(1))
            .edit(vec![rect(1, 0, 0)], id(1))
            .unwrap()
            .to_proto();
        wire.machines[0].blocked = blocked;
        let mut bytes = Vec::new();
        encode_frame(&wire, &mut bytes);
        let (decoded, _) = decode_frame::<v1::Layout>(&bytes, FrameKind::Control)
            .unwrap()
            .unwrap();

        // Act
        let layout = Layout::try_from(&decoded).unwrap();

        // Assert
        assert_eq!(u32::from(layout.placements()[0].blocked), expected);
        assert_eq!(layout.to_proto().machines[0].blocked, expected);
    }
}

#[test]
fn toggle_blocked_each_segment_flips_exactly_one_bit() {
    // Arrange
    for (index, edge) in [Edge::Left, Edge::Top, Edge::Right, Edge::Bottom]
        .into_iter()
        .enumerate()
    {
        for third in 0..3 {
            for before in [0, u16::MAX] {
                let mut placement = Placement {
                    blocked: before,
                    ..rect(1, 0, 0)
                };

                // Act
                placement.toggle_blocked(edge, third);

                // Assert
                assert_eq!(
                    placement.blocked,
                    before ^ (1 << (index * 3 + usize::from(third)))
                );
                assert_eq!(placement.is_blocked(edge, third), before == 0);
            }
        }
    }
}

#[test]
fn toggle_blocked_invalid_segment_leaves_bits_unchanged() {
    // Arrange
    for (edge, third) in [
        (Edge::Left, 3),
        (Edge::Bottom, u8::MAX),
        (Edge::Unspecified, 0),
    ] {
        let mut placement = Placement {
            blocked: u16::MAX,
            ..rect(1, 0, 0)
        };

        // Act
        placement.toggle_blocked(edge, third);

        // Assert
        assert_eq!(placement.blocked, u16::MAX);
        assert!(!placement.is_blocked(edge, third));
    }
}
