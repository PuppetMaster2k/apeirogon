// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::config::{Config, Modifier};
use a7n_proto::v1::{ScrollEvent, ScrollUnit};

use super::*;
use crate::command::Command;

fn modifier(down: bool, modifier: Modifier) -> InputEvent {
    InputEvent {
        event: Some(input_event::Event::Key(KeyEvent {
            usage: modifier.usage(),
            down,
            character: String::new(),
        })),
    }
}

fn save(peer: &Peer, text: &str) {
    Config::parse(text)
        .unwrap()
        .save(&peer.dir.path().join("config.toml"))
        .unwrap();
}

#[tokio::test]
async fn reload_config_denied_owner_releases_keys_and_returns_owner_home() {
    // Arrange
    let (a, b, mut a_input, mut b_input) = pair().await;
    drive(&mut a_input, &mut b_input).await;
    a_input
        .capture
        .send(Captured::Input(key(true)))
        .await
        .unwrap();
    assert!(matches!(b_input.injected().await, Inject::Event(event) if event == key(true)));
    save(&b, &format!("[peers.{}]\nallow_drive = false\n", a.machine));

    // Act
    b.command(Command::ReloadConfig).await.unwrap();

    // Assert
    assert!(matches!(b_input.injected().await, Inject::Event(event) if event == key(false)));
    b_input.grab(false).await;
    // Only Accepts(false) is sent by reload; receiving it must end A's drive.
    a_input.grab(false).await;
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn reload_config_invalid_file_keeps_previous_input_settings() {
    // Arrange
    let (a, b, mut a_input, mut b_input) = pair().await;
    drive(&mut a_input, &mut b_input).await;
    std::fs::write(b.dir.path().join("config.toml"), "[input]\nspeed = 0\n").unwrap();

    // Act
    let result = b.command(Command::ReloadConfig).await;

    // Assert
    assert!(matches!(result, Err(DaemonError::Config(_))));
    a_input
        .capture
        .send(Captured::Pointer {
            x: 50,
            y: 50,
            dx: 4,
            dy: -6,
        })
        .await
        .unwrap();
    assert!(matches!(
        b_input.injected().await,
        Inject::Motion { dx: 4, dy: -6 }
    ));
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn forward_capture_reload_mid_press_preserves_remap_until_next_take() {
    // Arrange
    let (a, b, mut a_input, mut b_input) = configured_pair(|a, b| {
        a.config
            .set(
                &format!("peers.{}.modifier_remap.left_meta", b.identity.machine_id),
                "left_ctrl",
            )
            .unwrap();
    })
    .await;
    drive(&mut a_input, &mut b_input).await;
    a_input
        .capture
        .send(Captured::Input(modifier(true, Modifier::LeftMeta)))
        .await
        .unwrap();
    let pressed = b_input.injected().await;
    save(
        &a,
        &format!(
            "[peers.{}]\nmodifier_remap = {{ left_meta = \"left_alt\" }}\n",
            b.machine
        ),
    );

    // Act
    a.command(Command::ReloadConfig).await.unwrap();
    a_input
        .capture
        .send(Captured::Input(modifier(false, Modifier::LeftMeta)))
        .await
        .unwrap();
    let released = b_input.injected().await;

    // Assert
    assert!(matches!(pressed, Inject::Event(event) if event == modifier(true, Modifier::LeftCtrl)));
    assert!(
        matches!(released, Inject::Event(event) if event == modifier(false, Modifier::LeftCtrl))
    );
    b_input.feedback.send(pointer(false)).await.unwrap();
    a_input.grab(false).await;
    b_input.grab(false).await;
    a_input.cross(true).await;
    a_input.grab(true).await;
    assert!(matches!(b_input.injected().await, Inject::Warp { .. }));
    a_input
        .capture
        .send(Captured::Input(modifier(true, Modifier::LeftMeta)))
        .await
        .unwrap();
    assert!(
        matches!(b_input.injected().await, Inject::Event(event) if event == modifier(true, Modifier::LeftAlt))
    );
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn reload_config_input_settings_apply_scrolling_and_receiver_speed() {
    // Arrange
    let (a, b, mut a_input, mut b_input) = pair().await;
    drive(&mut a_input, &mut b_input).await;
    save(&a, "[input]\nnatural_scrolling = true\n");
    save(&b, "[input]\nspeed = 0.5\n");
    let scroll = InputEvent {
        event: Some(input_event::Event::Scroll(ScrollEvent {
            dx: 2.0,
            dy: -3.0,
            unit: ScrollUnit::Line.into(),
        })),
    };

    // Act
    a.command(Command::ReloadConfig).await.unwrap();
    b.command(Command::ReloadConfig).await.unwrap();
    a_input
        .capture
        .send(Captured::Pointer {
            x: 50,
            y: 50,
            dx: 8,
            dy: -6,
        })
        .await
        .unwrap();
    a_input.capture.send(Captured::Input(scroll)).await.unwrap();

    // Assert
    assert!(matches!(
        b_input.injected().await,
        Inject::Motion { dx: 4, dy: -3 }
    ));
    // A reliable batch may repair motion before delivering scroll.
    loop {
        if let Inject::Event(event) = b_input.injected().await {
            assert!(matches!(
                event.event,
                Some(input_event::Event::Scroll(ScrollEvent {
                    dx: -2.0,
                    dy: 3.0,
                    ..
                }))
            ));
            break;
        }
    }
    a.stop().await;
    b.stop().await;
}
