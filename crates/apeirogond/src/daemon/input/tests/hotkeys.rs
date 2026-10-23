// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

const MODIFIERS: [u32; 3] = [0x0007_00e0, 0x0007_00e2, 0x0007_00e1];
const HOME: u32 = 0x0007_004a;
const TWO: u32 = 0x0007_001f;
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

async fn press(input: &FakeInput, usage: u32) {
    input
        .capture
        .send(Captured::Input(key_event(usage, true)))
        .await
        .unwrap();
}

async fn modifiers(input: &FakeInput) {
    for usage in MODIFIERS {
        press(input, usage).await;
    }
}

#[tokio::test]
async fn captured_home_chord_returns_home_ungrabs_and_consumes_key() {
    // Arrange
    let (a, b, mut a_input, mut b_input) = pair().await;
    drive(&mut a_input, &mut b_input).await;
    modifiers(&a_input).await;
    for usage in MODIFIERS {
        assert!(
            matches!(b_input.injected().await, Inject::Event(event) if event == key_event(usage, true))
        );
    }

    // Act
    press(&a_input, HOME).await;

    // Assert
    a_input.grab(false).await;
    b_input.grab(false).await;
    // Let-go releases only the three forwarded modifiers, never the Home key.
    for _ in MODIFIERS {
        let Inject::Event(event) = b_input.injected().await else {
            panic!("expected modifier release");
        };
        assert!(
            matches!(event.event, Some(input_event::Event::Key(key)) if !key.down && MODIFIERS.contains(&key.usage))
        );
    }
    assert!(b_input.inject.try_recv().is_err());
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn captured_jump_locked_after_edge_takes_peer_without_warp() {
    // Arrange
    let (a, b, mut a_input, mut b_input) = configured_pair(|a, _| {
        a.input.focus.set_locked(true);
    })
    .await;
    a_input.cross(true).await;
    modifiers(&a_input).await;

    // Act
    press(&a_input, TWO).await;

    // Assert
    a_input.grab(true).await;
    b_input.grab(true).await;
    a_input
        .capture
        .send(Captured::Input(key(true)))
        .await
        .unwrap();
    assert!(matches!(b_input.injected().await, Inject::Event(event) if event == key(true)));
    a.stop().await;
    b.stop().await;
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
