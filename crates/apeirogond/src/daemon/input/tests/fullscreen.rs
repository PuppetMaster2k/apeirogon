// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[tokio::test]
async fn cross_edge_fullscreen_local_and_driven_push_discards_resistance() {
    // Arrange
    for driven in [false, true] {
        let (mut daemon, _dir, _input) = daemon(true);
        let bounds = daemon.input.capture.as_ref().unwrap().bounds;
        daemon.input.resistance = EdgeResistance::new(10);
        daemon.input.resistance.push(Edge::Right, 8);
        daemon.input.fullscreen_hold = FullscreenHold::new(true);
        daemon.input.fullscreen_hold.holds(true, || true);

        // Act
        daemon.cross_edge(bounds, 99, 50, 1, 0, driven).await;
        let crossed = daemon.input.resistance.push(Edge::Right, 1);

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
        daemon.cross_edge(bounds, x, 50, 1, 0, driven).await;
        let result = daemon.input.fullscreen_hold.holds(true, || false);

        // Assert
        assert!(!result);
    }
}

#[tokio::test]
async fn reload_config_fullscreen_hold_rebuilds_enabled_and_clears_cache() {
    // Arrange
    let (mut daemon, dir, _input) = daemon(true);
    daemon.config_path = dir.path().join("config.toml");
    std::fs::write(&daemon.config_path, "[input]\nfullscreen_hold = true\n").unwrap();

    // Act
    daemon.reload_config().await.unwrap();
    let enabled = daemon.input.fullscreen_hold.holds(true, || true);
    std::fs::write(&daemon.config_path, "[input]\nfullscreen_hold = false\n").unwrap();
    daemon.reload_config().await.unwrap();
    let disabled = daemon
        .input
        .fullscreen_hold
        .holds(true, || panic!("disabled must not query"));

    // Assert
    assert!(enabled);
    assert!(!disabled);
}
