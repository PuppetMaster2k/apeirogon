// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

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
fn edge_push_outside_desktop_clamps_point_and_preserves_outward_motion() {
    // Arrange
    let bounds = (-100, -200, 201, 401);
    let cases = [
        (-103, 0, -7, 0, Some((Edge::Left, 7, 0.5))),
        (103, 0, 7, 0, Some((Edge::Right, 7, 0.5))),
        (0, -203, 0, -7, Some((Edge::Top, 7, 0.5))),
        (0, 203, 0, 7, Some((Edge::Bottom, 7, 0.5))),
        (-103, 0, 7, 0, None),
        (103, 0, -7, 0, None),
        (0, -203, 0, 7, None),
        (0, 203, 0, -7, None),
        (-103, -1000, -7, 0, Some((Edge::Left, 7, 0.0))),
        (103, 1000, 7, 0, Some((Edge::Right, 7, 1.0))),
        (-1000, -203, 0, -7, Some((Edge::Top, 7, 0.0))),
        (1000, 203, 0, 7, Some((Edge::Bottom, 7, 1.0))),
        (-103, -203, -7, -7, Some((Edge::Left, 7, 0.0))),
        (103, -203, 7, -7, Some((Edge::Right, 7, 0.0))),
        (-103, 203, -7, 7, Some((Edge::Left, 7, 1.0))),
        (103, 203, 7, 7, Some((Edge::Right, 7, 1.0))),
        (-103, -203, 7, 7, None),
        (0, 0, -7, 0, None),
        (-100, 0, i32::MIN, 0, Some((Edge::Left, i32::MAX, 0.5))),
    ];
    for (x, y, dx, dy, expected) in cases {
        // Act
        let push = edge_push(bounds, x, y, dx, dy);

        // Assert
        assert_eq!(push, expected, "point ({x}, {y}), motion ({dx}, {dy})");
    }
}

#[test]
fn corner_push_near_edge_end_requires_diagonal_outward_motion() {
    // Arrange
    let bounds = (-10, -20, 101, 101);
    let cases = [
        (-10, -12, -1, -1, Some((Edge::Left, Corner::TopLeft))),
        (-10, -11, -1, -1, None),
        (90, -12, 1, -1, Some((Edge::Right, Corner::TopRight))),
        (90, 72, 1, 1, Some((Edge::Right, Corner::BottomRight))),
        (90, 71, 1, 1, None),
        (-10, 72, -1, 1, Some((Edge::Left, Corner::BottomLeft))),
        (-2, -20, -1, -1, Some((Edge::Top, Corner::TopLeft))),
        (-1, -20, -1, -1, None),
        (82, -20, 1, -1, Some((Edge::Top, Corner::TopRight))),
        (82, 80, 1, 1, Some((Edge::Bottom, Corner::BottomRight))),
        (81, 80, 1, 1, None),
        (-2, 80, -1, 1, Some((Edge::Bottom, Corner::BottomLeft))),
        (90, 80, 1, 0, None),
        (90, 80, 1, -1, None),
        (90, 80, -1, -1, None),
        (89, 79, 1, 1, None),
        (-13, -23, -3, -3, Some((Edge::Left, Corner::TopLeft))),
        (93, -23, 3, -3, Some((Edge::Right, Corner::TopRight))),
        (-13, 83, -3, 3, Some((Edge::Left, Corner::BottomLeft))),
        (93, 83, 3, 3, Some((Edge::Right, Corner::BottomRight))),
        (-13, -23, 3, 3, None),
    ];
    for (x, y, dx, dy, expected) in cases {
        // Act
        let corner = corner_push(bounds, x, y, dx, dy);

        // Assert
        assert_eq!(corner, expected, "pointer ({x}, {y}), motion ({dx}, {dy})");
    }
}
