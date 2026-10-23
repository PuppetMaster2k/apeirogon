// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn apply_unit_speed_preserves_exact_deltas() {
    // Arrange
    let mut scale = MotionScale::new(1.0);
    let deltas = [(1, -1), (i32::MIN, i32::MAX), (0, 0)];

    // Act
    let scaled = deltas.map(|(dx, dy)| scale.apply(dx, dy));

    // Assert
    assert_eq!(scaled, deltas);
}

#[test]
fn apply_half_speed_retains_positive_and_negative_subpixels() {
    // Arrange
    let mut scale = MotionScale::new(0.5);

    // Act
    let scaled = [(1, -1), (1, -1), (-1, 1), (-1, 1)].map(|(dx, dy)| scale.apply(dx, dy));

    // Assert
    assert_eq!(scaled, [(0, 0), (1, -1), (0, 0), (-1, 1)]);
}

#[test]
fn apply_stale_or_duplicate_sequence_drops_motion() {
    // Arrange
    let mut tracker = MotionTracker::default();
    tracker.apply(2, 10, 20);

    // Act
    let deltas = [(1, 5, 5), (2, 10, 20), (3, 12, 17)].map(|(seq, x, y)| tracker.apply(seq, x, y));

    // Assert
    assert_eq!(deltas, [None, None, Some((2, -3))]);
}

#[test]
fn apply_sequence_and_counters_wrapped_returns_small_delta() {
    // Arrange
    let mut tracker = MotionTracker {
        seq: u32::MAX,
        x: i32::MAX - 2,
        y: i32::MIN + 2,
    };

    // Act
    let movement = tracker.apply(0, i32::MIN + 2, i32::MAX - 2);

    // Assert
    assert_eq!(movement, Some((5, -5)));
}

#[test]
fn resync_lost_datagram_repairs_motion_before_later_updates() {
    // Arrange
    let mut tracker = MotionTracker::default();
    tracker.apply(1, 4, 2);

    // Act
    let deltas = [
        tracker.resync(3, 10, 7),
        tracker.apply(2, 7, 4),
        tracker.resync(3, 11, 8),
        tracker.resync(3, 11, 8),
        tracker.resync(2, 7, 4),
        tracker.apply(4, 12, 10),
    ];

    // Assert
    assert_eq!(
        deltas,
        [Some((6, 5)), None, Some((1, 1)), None, None, Some((1, 2))]
    );
}

#[test]
fn push_wrapped_counters_and_sequence_reset_on_new_route() {
    // Arrange
    let mut counter = MotionCounter {
        seq: u32::MAX,
        x: i32::MAX,
        y: i32::MIN,
    };

    // Act
    let wrapped = counter.push(1, -1);
    counter.reset();
    let first = counter.push(2, 3);

    // Assert
    assert_eq!(
        (wrapped.motion_seq, wrapped.x, wrapped.y),
        (0, i32::MIN, i32::MAX)
    );
    assert_eq!((first.motion_seq, first.x, first.y), (1, 2, 3));
}

#[test]
fn edge_push_edges_inward_and_corner_selects_outward_edge() {
    // Arrange
    let bounds = (-10, -20, 101, 101);
    let cases = [
        (-10, 30, -4, 0, Some((Edge::Left, 4, 0.5))),
        (90, 30, 4, 0, Some((Edge::Right, 4, 0.5))),
        (40, -20, 0, -4, Some((Edge::Top, 4, 0.5))),
        (40, 80, 0, 4, Some((Edge::Bottom, 4, 0.5))),
        (-10, 30, 4, 0, None),
        (90, 30, -4, 0, None),
        (40, -20, 0, 4, None),
        (40, 80, 0, -4, None),
        (40, 30, 4, 4, None),
        (-10, -20, -4, -4, Some((Edge::Left, 4, 0.0))),
        (90, 80, 4, 4, Some((Edge::Right, 4, 1.0))),
        (-11, 30, -4, 0, None),
        (-10, 30, i32::MIN, 0, Some((Edge::Left, i32::MAX, 0.5))),
    ];

    for (x, y, dx, dy, expected) in cases {
        // Act
        let push = edge_push(bounds, x, y, dx, dy);

        // Assert
        assert_eq!(push, expected);
    }
}

#[test]
fn edge_push_empty_or_single_pixel_bounds_handles_degenerate_desktops() {
    // Arrange
    let cases = [
        ((0, 0, 0, 10), None),
        ((0, 0, 10, 0), None),
        ((0, 0, 1, 1), Some((Edge::Left, 1, 0.0))),
        ((i32::MIN, 0, u32::MAX, 1), Some((Edge::Right, 1, 0.0))),
    ];

    for (bounds, expected) in cases {
        let (x, dx) = if bounds.2 == u32::MAX {
            (i32::MAX - 1, 1)
        } else {
            (0, -1)
        };

        // Act
        let push = edge_push(bounds, x, 0, dx, 0);

        // Assert
        assert_eq!(push, expected);
    }
}
