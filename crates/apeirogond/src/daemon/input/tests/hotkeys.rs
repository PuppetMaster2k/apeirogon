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
