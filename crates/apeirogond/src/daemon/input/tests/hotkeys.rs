// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

const SCROLL_LOCK: u32 = 0x0007_0047;

fn key_event(usage: u32, down: bool) -> InputEvent {
    InputEvent {
        event: Some(input_event::Event::Key(KeyEvent {
            usage,
            down,
            character: String::new(),
        })),
    }
}

#[tokio::test]
async fn reload_config_hotkeys_rebuilds_matcher_with_new_binding() {
    // Arrange
    let (mut daemon, dir, _input) = daemon(true);
    let config = a7n_ipc::config::Config::parse("[hotkeys]\nlock = 'escape'\n").unwrap();
    let path = dir.path().join("config.toml");
    daemon = daemon.with_config(config, path.clone());
    daemon
        .captured(Some(Captured::Input(key_event(0x0007_0029, true))))
        .await;
    assert!(daemon.input.focus.is_locked());
    a7n_ipc::config::Config::parse(
        "[hotkeys]\nlock = 'scroll_lock'\nhome = ['ctrl+alt+shift+home']\n",
    )
    .unwrap()
    .save(&path)
    .unwrap();

    // Act
    daemon.reload_config().await.unwrap();
    daemon
        .captured(Some(Captured::Input(key_event(SCROLL_LOCK, true))))
        .await;

    // Assert
    assert!(!daemon.input.focus.is_locked());
}

#[tokio::test]
async fn captured_recording_returns_trigger_name_and_rejects_text() {
    // Arrange
    for (usage, expected) in [
        (0x0007_0068, Ok("f13")),
        (0x0007_00e6, Ok("right_alt")),
        (
            0x0007_0004,
            Err("that key types text; pick a modifier, F-key, Scroll Lock, Pause or Menu"),
        ),
    ] {
        let (mut daemon, _dir, _input) = daemon(true);
        let (reply, recorded) = oneshot::channel();
        daemon
            .command(crate::command::Command::RecordTriggerKey(reply))
            .await
            .unwrap();
        let event = Captured::Input(InputEvent {
            event: Some(input_event::Event::Key(KeyEvent {
                usage,
                down: true,
                character: String::new(),
            })),
        });

        // Act
        daemon.captured(Some(event)).await;
        let result = recorded.await.unwrap().map_err(|error| error.to_string());

        // Assert
        assert_eq!(
            result.as_deref(),
            expected.map_err(str::to_owned).as_deref()
        );
        assert!(daemon.input.hotkeys.next_deadline().is_none());
        assert_eq!(daemon.input.focus.cursor(), Cursor::Home);
    }
}

#[tokio::test(start_paused = true)]
async fn captured_shake_while_driving_returns_home() {
    // Arrange
    let (mut daemon, _dir, input) = daemon(true);
    let mut input = input.unwrap();
    let peer = MachineId::try_from(&[42; MachineId::LEN][..]).unwrap();
    daemon.take_peer(peer, None, now());
    let actions = daemon
        .input
        .focus
        .on_accept(peer, &a7n_proto::v1::Accept { route: 1 });
    daemon.input_actions(actions).await;
    input.grab(true).await;

    // Act
    for dx in [80, -80, 80, -80, 80, -80, 80] {
        daemon
            .captured(Some(Captured::Pointer {
                x: 50,
                y: 50,
                dx,
                dy: 0,
            }))
            .await;
    }

    // Assert
    assert_eq!(daemon.input.focus.cursor(), Cursor::Home);
    input.grab(false).await;
}
