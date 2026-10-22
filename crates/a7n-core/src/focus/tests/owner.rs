// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn on_accept_pending_take_drives_target() {
    // Arrange
    let now = Instant::now();
    let mut focus = Focus::new(id(1));
    let sent = focus.take(request(2), now);

    // Act
    let actions = focus.on_accept(id(2), &Accept { route: 1 });

    // Assert
    assert!(has_let_go(&sent, 2, 2));
    assert_eq!(focus.cursor(), Cursor::Driving(id(2)));
    assert_eq!(actions, vec![Action::Cursor(Cursor::Driving(id(2)))]);
    assert_eq!(focus.next_deadline(), None);
}

#[test]
fn on_refuse_pending_take_restores_previous_target() {
    // Arrange
    let now = Instant::now();
    for previous_remote in [false, true] {
        let mut focus = if previous_remote {
            driving(now)
        } else {
            Focus::new(id(1))
        };
        let sent = focus.take(request(3), now);
        let Action::Send {
            body: Body::Focus(take),
            ..
        } = &sent[0]
        else {
            panic!("take required")
        };

        // Act
        let actions = focus.on_refuse(
            id(3),
            &Refuse {
                route: take.route,
                reason: RefuseReason::Owned.into(),
                other: id(2).as_bytes().to_vec(),
            },
        );

        // Assert
        if previous_remote {
            assert_eq!(focus.cursor(), Cursor::Driving(id(2)));
            assert!(
                matches!(&actions[0], Action::Send { peer, body: Body::Focus(f) } if *peer == id(2) && f.stay && f.route > take.route && f.landing.is_none())
            );
        } else {
            assert_eq!(focus.cursor(), Cursor::Home);
            assert_eq!(actions, vec![Action::Cursor(Cursor::Home)]);
        }
    }
}

#[test]
fn poll_take_timeout_lets_go_both_targets() {
    // Arrange
    let now = Instant::now();
    let mut focus = driving(now);
    focus.take(request(3), now);

    // Act
    let actions = focus.poll(now + limits::TAKE_TIMEOUT);

    // Assert
    assert_eq!(focus.cursor(), Cursor::Home);
    assert!(has_let_go(&actions, 2, 1));
    assert!(has_let_go(&actions, 3, 1));
}

#[test]
fn on_accept_after_timeout_is_ignored() {
    // Arrange
    let now = Instant::now();
    let mut focus = Focus::new(id(1));
    focus.take(request(2), now);
    focus.poll(now + limits::TAKE_TIMEOUT);

    // Act
    let actions = focus.on_accept(id(2), &Accept { route: 1 });

    // Assert
    assert_eq!(actions, Vec::new());
    assert_eq!(focus.cursor(), Cursor::Home);
}

#[test]
fn on_handover_onward_takes_directly_then_lets_go_previous() {
    // Arrange
    let now = Instant::now();
    let mut focus = driving(now);
    let handover = Handover {
        route: 1,
        next: id(3).as_bytes().to_vec(),
        ..Default::default()
    };

    // Act
    let actions = focus.on_handover(id(2), &handover, now);
    let accepted = focus.on_accept(id(3), &Accept { route: 2 });

    // Assert
    assert!(has_let_go(&actions, 3, 3));
    assert!(!has_let_go(&actions, 2, 3));
    assert!(has_let_go(&accepted, 2, 3));
    assert_eq!(focus.cursor(), Cursor::Driving(id(3)));
}

#[test]
fn on_handover_home_lets_go_target() {
    // Arrange
    let now = Instant::now();
    let mut focus = driving(now);
    let handover = Handover {
        route: 1,
        next: id(1).as_bytes().to_vec(),
        ..Default::default()
    };

    // Act
    let actions = focus.on_handover(id(2), &handover, now);

    // Assert
    assert!(has_let_go(&actions, 2, 1));
    assert_eq!(focus.cursor(), Cursor::Home);
}

#[test]
fn on_handover_taken_over_goes_home_without_let_go() {
    // Arrange
    let now = Instant::now();
    let mut focus = driving(now);
    let handover = Handover {
        route: 1,
        next: id(1).as_bytes().to_vec(),
        taken_over_by: id(3).as_bytes().to_vec(),
        ..Default::default()
    };

    // Act
    let actions = focus.on_handover(id(2), &handover, now);

    // Assert
    assert_eq!(actions, vec![Action::Cursor(Cursor::Home)]);
    assert_eq!(focus.cursor(), Cursor::Home);
}

#[test]
fn on_ack_locked_returns_home_with_let_go() {
    // Arrange
    let now = Instant::now();
    let mut focus = driving(now);

    // Act
    let actions = focus.on_ack(
        id(2),
        &Ack {
            locked: true,
            ..Default::default()
        },
    );

    // Assert
    assert_eq!(focus.cursor(), Cursor::Home);
    assert!(has_let_go(&actions, 2, 1));
}

#[test]
fn take_pending_ignores_further_takes_but_allows_home() {
    // Arrange
    let now = Instant::now();
    let mut focus = Focus::new(id(1));
    focus.take(request(2), now);

    // Act
    let ignored = focus.take(request(3), now);
    let home = focus.take(request(1), now);

    // Assert
    assert_eq!(ignored, Vec::new());
    assert!(has_let_go(&home, 2, 1));
    assert_eq!(focus.cursor(), Cursor::Home);
}

#[test]
fn on_handover_wrong_route_or_unreachable_peer_is_ignored() {
    // Arrange
    let now = Instant::now();
    let mut focus = driving(now);
    let cases = [(0, 3), (1, 4), (2, 3)];
    for (route, next) in cases {
        let handover = Handover {
            route,
            next: id(next).as_bytes().to_vec(),
            ..Default::default()
        };

        // Act
        let actions = focus.on_handover(id(2), &handover, now);

        // Assert
        assert_eq!(actions, Vec::new());
        assert_eq!(focus.cursor(), Cursor::Driving(id(2)));
    }
}

#[test]
fn on_link_lost_pending_target_returns_home_and_releases_previous() {
    // Arrange
    let now = Instant::now();
    let mut focus = driving(now);
    focus.take(request(3), now);

    // Act
    let actions = focus.on_link_lost(id(3));

    // Assert
    assert_eq!(focus.cursor(), Cursor::Home);
    assert!(has_let_go(&actions, 2, 1));
    assert!(!has_let_go(&actions, 3, 1));
}

#[test]
fn take_replies_mismatched_peer_or_route_are_ignored() {
    // Arrange
    let now = Instant::now();
    let mut focus = Focus::new(id(1));
    focus.take(request(2), now);
    for (peer, route) in [(3, 1), (2, 0), (2, 2)] {
        // Act
        let actions = focus.on_accept(id(peer), &Accept { route });
        let refused = focus.on_refuse(
            id(peer),
            &Refuse {
                route,
                ..Default::default()
            },
        );

        // Assert
        assert_eq!(actions, Vec::new());
        assert_eq!(refused, Vec::new());
        assert_eq!(focus.cursor(), Cursor::Home);
        assert!(focus.next_deadline().is_some());
    }
}
