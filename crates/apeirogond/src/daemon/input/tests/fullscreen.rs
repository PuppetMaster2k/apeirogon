// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[tokio::test]
async fn cross_edge_fullscreen_local_and_driven_push_discards_resistance() {
    // Arrange
    for driven in [false, true] {
        let (mut daemon, _dir, _input) = daemon(true);
        let bounds = daemon.input.capture.as_ref().unwrap().bounds;
        daemon.input.resistance = EdgeResistance::new(10);
        daemon.input.resistance.push(Edge::Right, 8, now());
        daemon.input.fullscreen_hold = FullscreenHold::new(true);
        daemon.input.fullscreen_hold.holds(true, || true);

        // Act
        daemon
            .cross_edge(bounds, &[bounds], (99, 50), (1, 0), driven)
            .await;
        let crossed = daemon.input.resistance.push(Edge::Right, 1, now());

        // Assert
        assert!(!crossed);
        assert!(
            daemon
                .input
                .fullscreen_hold
                .holds(true, || panic!("push must stay cached"))
        );
    }
}

#[tokio::test]
async fn cross_edge_blocked_push_clears_fullscreen_cache() {
    // Arrange
    for (button, paused, locked, x, driven) in [
        (true, false, false, 99, false),
        (true, false, false, 50, false),
        (false, true, false, 99, false),
        (false, true, false, 99, true),
        (false, false, true, 99, false),
    ] {
        let (mut daemon, _dir, _input) = daemon(true);
        let bounds = daemon.input.capture.as_ref().unwrap().bounds;
        daemon.input.fullscreen_hold = FullscreenHold::new(true);
        daemon.input.fullscreen_hold.holds(true, || true);
        daemon.input.paused = paused;
        daemon.input.focus.set_locked(locked);
        if button {
            daemon.input.buttons.apply(&a7n_proto::v1::ButtonEvent {
                button: a7n_proto::v1::Button::Left.into(),
                down: true,
            });
        }

        // Act
        daemon
            .cross_edge(bounds, &[bounds], (x, 50), (1, 0), driven)
            .await;
        let result = daemon.input.fullscreen_hold.holds(true, || false);

        // Assert
        assert!(!result);
    }
}
