// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[tokio::test]
async fn captured_listed_inputs_driving_peer_forwards_only_unlisted_key() {
    // Arrange
    let (a, b, mut a_input, mut b_input) = configured_pair(|a, _| {
        let config =
            a7n_ipc::config::Config::parse("[input]\nlocal_keys = ['back', 'volume_up']").unwrap();
        a.input.local_inputs = config.local_inputs().unwrap();
        a.config = config;
    })
    .await;
    a_input.cross(true).await;
    a_input.grab(true).await;
    b_input.grab(true).await;
    assert!(matches!(b_input.injected().await, Inject::Warp { .. }));
    let listed = [
        input_event::Event::Key(KeyEvent {
            usage: 0x000c_00e9,
            down: true,
            character: String::new(),
        }),
        input_event::Event::Button(a7n_proto::v1::ButtonEvent {
            button: a7n_proto::v1::Button::Back.into(),
            down: true,
        }),
    ];

    // Act
    for event in listed {
        for down in [true, false] {
            let mut event = event.clone();
            match &mut event {
                input_event::Event::Key(key) => key.down = down,
                input_event::Event::Button(button) => button.down = down,
                _ => unreachable!(),
            }
            a_input
                .capture
                .send(Captured::Input(InputEvent { event: Some(event) }))
                .await
                .unwrap();
        }
    }
    a_input
        .capture
        .send(Captured::Input(key(true)))
        .await
        .unwrap();

    // Assert
    assert!(matches!(b_input.injected().await, Inject::Event(event) if event == key(true)));
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn captured_listed_hotkey_keeps_hotkey_priority() {
    // Arrange
    let (mut daemon, dir, _input) = daemon(true);
    let config = a7n_ipc::config::Config::parse(
        "[input]\nlocal_keys = ['escape']\n[hotkeys]\nlock = 'escape'",
    )
    .unwrap();
    daemon = daemon.with_config(config, dir.path().join("config.toml"));
    let event = InputEvent {
        event: Some(input_event::Event::Key(KeyEvent {
            usage: 0x0007_0029,
            down: true,
            character: String::new(),
        })),
    };

    // Act
    daemon.captured(Some(Captured::Input(event))).await;

    // Assert
    assert!(daemon.input.focus.is_locked());
}
