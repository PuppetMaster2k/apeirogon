// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn on_focus_older_or_equal_route_is_ignored_per_owner() {
    // Arrange
    let now = Instant::now();
    for route in [9, 10] {
        let mut focus = Focus::new(id(1));
        focus.on_focus(id(2), &wire(10), true, now);

        // Act
        let ignored = focus.on_focus(id(2), &wire(route), true, now);
        let different_owner = focus.on_focus(id(3), &wire(1), true, now + limits::IDLE_TAKEOVER);

        // Assert
        assert_eq!(ignored, Vec::new());
        assert!(different_owner.iter().any(|a| matches!(
            a,
            Action::Send {
                body: Body::Accept(Accept { route: 1 }),
                ..
            }
        )));
    }
}

#[test]
fn on_focus_idle_owner_takeover_releases_held_and_notifies_previous() {
    // Arrange
    let now = Instant::now();
    let mut focus = Focus::new(id(1));
    focus.on_focus(id(2), &wire(1), true, now);
    focus.on_input_event(id(2), Route(1), &held_key(), now);

    // Act
    let actions = focus.on_focus(id(3), &wire(1), true, now + limits::IDLE_TAKEOVER);

    // Assert
    assert!(released_key(&actions));
    assert!(matches!(&actions[0], Action::Release(_)));
    assert!(actions.iter().any(|a| matches!(a, Action::Send { peer, body: Body::Handover(h) } if *peer == id(2) && h.route == 1 && h.taken_over_by == id(3).as_bytes() && h.next == id(2).as_bytes())));
    assert!(focus.on_remote_input(id(3), Route(1), now + limits::IDLE_TAKEOVER));
}

#[test]
fn on_focus_active_owner_refuses_owned() {
    // Arrange
    let now = Instant::now();
    let mut focus = Focus::new(id(1));
    focus.on_focus(id(2), &wire(1), true, now);

    // Act
    let actions = focus.on_focus(id(3), &wire(2), true, now);

    // Assert
    assert!(
        matches!(&actions[..], [Action::Send { peer, body: Body::Refuse(r) }] if *peer == id(3) && r.reason == i32::from(RefuseReason::Owned) && r.other == id(2).as_bytes())
    );
}

#[test]
fn on_local_input_remote_owner_active_drops_then_idle_takes_over() {
    // Arrange
    let now = Instant::now();
    let mut focus = Focus::new(id(1));
    focus.on_focus(id(2), &wire(1), true, now);
    focus.on_input_event(id(2), Route(1), &held_key(), now);

    // Act
    let active = focus.on_local_input(now);
    let idle = focus.on_local_input(now + limits::IDLE_TAKEOVER);

    // Assert
    assert_eq!(active, (false, Vec::new()));
    assert!(idle.0);
    assert!(released_key(&idle.1));
    assert!(idle.1.iter().any(|a| matches!(a, Action::Send { body: Body::Handover(h), .. } if h.taken_over_by == id(1).as_bytes())));
    assert!(!focus.on_remote_input(id(2), Route(1), now));
}

#[test]
fn on_link_lost_current_owner_releases_held() {
    // Arrange
    let now = Instant::now();
    let mut focus = Focus::new(id(1));
    focus.on_focus(id(2), &wire(1), true, now);
    focus.on_input_event(id(2), Route(1), &held_key(), now);

    // Act
    let actions = focus.on_link_lost(id(2));

    // Assert
    assert!(released_key(&actions));
    assert!(!focus.on_remote_input(id(2), Route(1), now));
}

#[test]
fn on_focus_drive_denied_refuses_not_allowed() {
    // Arrange
    let now = Instant::now();
    let mut focus = Focus::new(id(1));

    // Act
    let actions = focus.on_focus(id(2), &wire(1), false, now);

    // Assert
    assert!(
        matches!(&actions[..], [Action::Send { body: Body::Refuse(r), .. }] if r.reason == i32::from(RefuseReason::NotAllowed))
    );
    assert!(!focus.on_remote_input(id(2), Route(1), now));
}

#[test]
fn on_focus_malformed_target_or_reach_refuses_malformed() {
    // Arrange
    let now = Instant::now();
    for bad_target in [true, false] {
        let mut focus = Focus::new(id(1));
        let mut message = wire(1);
        if bad_target {
            message.target.clear();
        } else {
            message.reach.push(vec![0]);
        }

        // Act
        let actions = focus.on_focus(id(2), &message, true, now);

        // Assert
        assert!(
            matches!(&actions[..], [Action::Send { body: Body::Refuse(r), .. }] if r.reason == i32::from(RefuseReason::Malformed))
        );
    }
}

#[test]
fn on_focus_let_go_releases_and_rejects_delayed_take() {
    // Arrange
    let now = Instant::now();
    let mut focus = Focus::new(id(1));
    focus.on_focus(id(2), &wire(1), true, now);
    focus.on_input_event(id(2), Route(1), &held_key(), now);
    let mut message = wire(3);
    message.target = id(2).as_bytes().to_vec();

    // Act
    let released = focus.on_focus(id(2), &message, true, now);
    let late = focus.on_focus(id(2), &wire(2), true, now);

    // Assert
    assert!(released_key(&released));
    assert_eq!(late, Vec::new());
    assert!(!focus.on_remote_input(id(2), Route(1), now));
}

#[test]
fn on_focus_stay_rearm_preserves_held_input() {
    // Arrange
    let now = Instant::now();
    let mut focus = Focus::new(id(1));
    focus.on_focus(id(2), &wire(1), true, now);
    focus.on_input_event(id(2), Route(1), &held_key(), now);
    let mut message = wire(2);
    message.stay = true;

    // Act
    let actions = focus.on_focus(id(2), &message, true, now);
    let released = focus.on_link_lost(id(2));

    // Assert
    assert!(!released_key(&actions));
    assert!(released_key(&released));
}

#[test]
fn handover_unreachable_peer_is_ignored_but_home_allowed() {
    // Arrange
    let now = Instant::now();
    let mut focus = Focus::new(id(1));
    focus.on_focus(id(2), &wire(1), true, now);

    // Act
    let unknown = focus.handover(id(4), None);
    let reachable = focus.handover(id(3), None);
    let home = focus.handover(id(2), None);

    // Assert
    assert_eq!(unknown, Vec::new());
    assert!(
        matches!(&reachable[..], [Action::Send { peer, body: Body::Handover(h) }] if *peer == id(2) && h.next == id(3).as_bytes())
    );
    assert!(
        matches!(&home[..], [Action::Send { body: Body::Handover(h), .. }] if h.next == id(2).as_bytes())
    );
}

#[test]
fn on_remote_input_stale_route_does_not_refresh_idle_timer() {
    // Arrange
    let now = Instant::now();
    let mut focus = Focus::new(id(1));
    focus.on_focus(id(2), &wire(2), true, now);

    // Act
    let stale = focus.on_remote_input(id(2), Route(1), now + limits::IDLE_TAKEOVER);
    let takeover = focus.on_local_input(now + limits::IDLE_TAKEOVER);

    // Assert
    assert!(!stale);
    assert!(takeover.0);
}

#[test]
fn on_focus_recent_local_activity_refuses_busy() {
    // Arrange
    let now = Instant::now();
    let mut focus = Focus::new(id(1));
    focus.on_local_input(now);

    // Act
    let actions = focus.on_focus(id(2), &wire(1), true, now);

    // Assert
    assert!(
        matches!(&actions[..], [Action::Send { body: Body::Refuse(r), .. }] if r.reason == i32::from(RefuseReason::Busy) && r.other == id(1).as_bytes())
    );
}

#[test]
fn on_focus_after_owner_link_lost_accepts_low_route() {
    // Arrange
    let now = Instant::now();
    let mut focus = Focus::new(id(1));
    focus.on_focus(id(2), &wire(5), true, now);
    focus.on_link_lost(id(2));

    // Act
    let actions = focus.on_focus(id(2), &wire(1), true, now);

    // Assert
    assert!(actions.iter().any(|action| matches!(action,
        Action::Send { peer, body: Body::Accept(Accept { route: 1 }) } if *peer == id(2)
    )));
    assert!(focus.on_remote_input(id(2), Route(1), now));
}
