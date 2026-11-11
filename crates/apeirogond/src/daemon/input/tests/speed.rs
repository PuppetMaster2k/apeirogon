// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_proto::v1::{Focus, ScrollEvent, ScrollUnit};

use super::*;

async fn driven(daemon: &mut Daemon, peer: MachineId) {
    let actions = daemon.input.focus.on_focus(
        peer,
        &Focus {
            route: 1,
            target: daemon.identity.machine_id.as_bytes().to_vec(),
            ..Default::default()
        },
        true,
        now(),
    );
    daemon.input_actions(actions).await;
}

fn scroll_batch(dx: f32, dy: f32) -> InputBatch {
    InputBatch {
        route: 1,
        motion_seq: 1,
        motion_x: 4,
        motion_y: -4,
        events: vec![InputEvent {
            event: Some(Event::Scroll(ScrollEvent {
                dx,
                dy,
                unit: ScrollUnit::Line.into(),
            })),
        }],
        ..Default::default()
    }
}

#[tokio::test]
async fn receive_input_different_drivers_use_local_peer_speeds_and_reset_remainders() {
    // Arrange
    let (mut daemon, _dir, fake) = daemon(true);
    let mut fake = fake.unwrap();
    let a = MachineId::try_from(&[1; MachineId::LEN][..]).unwrap();
    let b = MachineId::try_from(&[2; MachineId::LEN][..]).unwrap();
    daemon.config.input.speed = 2.0;
    for (peer, pointer, scroll, reverse) in [(a, "0.25", "0.25", "true"), (b, "4", "4", "false")] {
        for (key, value) in [
            ("pointer_speed", pointer),
            ("scroll_speed", scroll),
            ("reverse_scroll", reverse),
        ] {
            daemon
                .config
                .set(&format!("peers.{peer}.{key}"), value)
                .unwrap();
        }
    }
    let mut received = Vec::new();

    // Act
    let unconfigured = MachineId::try_from(&[3; MachineId::LEN][..]).unwrap();
    for peer in [a, b, a, unconfigured] {
        driven(&mut daemon, peer).await;
        daemon.receive_input(peer, &scroll_batch(3.0, -3.0)).await;
        received.push((fake.injected().await, fake.injected().await));
        let actions = daemon.input.focus.on_link_lost(peer);
        daemon.input_actions(actions).await;
    }

    // Assert
    for ((motion, event), (delta, scroll)) in
        received
            .into_iter()
            .zip([(2, 0.0), (32, 12.0), (2, 0.0), (8, 3.0)])
    {
        assert!(matches!(motion, Inject::Motion { dx, dy } if (dx, dy) == (delta, -delta)));
        assert!(
            matches!(event, Inject::Event(InputEvent { event: Some(Event::Scroll(s)) }) if (s.dx, s.dy) == (scroll, -scroll))
        );
    }
}

#[tokio::test]
async fn apply_config_active_driver_updates_scales_and_clamps_global_product() {
    for (base, multiplier, expected) in [(10.0, "4", 400), (0.1, "0.25", 4)] {
        // Arrange
        let (mut daemon, _dir, fake) = daemon(true);
        let mut fake = fake.unwrap();
        let peer = MachineId::try_from(&[1; MachineId::LEN][..]).unwrap();
        driven(&mut daemon, peer).await;
        let mut config = daemon.config.clone();
        config.input.speed = base;
        config
            .set(&format!("peers.{peer}.pointer_speed"), multiplier)
            .unwrap();
        config
            .set(&format!("peers.{peer}.scroll_speed"), "0.5")
            .unwrap();
        config
            .set(&format!("peers.{peer}.reverse_scroll"), "true")
            .unwrap();
        let mut batch = scroll_batch(4.0, -4.0);
        batch.motion_x = 40;
        batch.motion_y = -40;

        // Act
        daemon.apply_config(config).await;
        daemon.receive_input(peer, &batch).await;

        // Assert
        assert!(
            matches!(fake.injected().await, Inject::Motion { dx, dy } if (dx, dy) == (expected, -expected))
        );
        assert!(
            matches!(fake.injected().await, Inject::Event(InputEvent { event: Some(Event::Scroll(s)) }) if (s.dx, s.dy) == (-2.0, 2.0))
        );
    }
}
