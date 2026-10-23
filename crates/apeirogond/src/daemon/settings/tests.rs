// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::{Ipv4Addr, SocketAddr};

use a7n_ipc::config::Config;
use tempfile::TempDir;

use super::*;
use crate::settings_store::SettingsStore;

fn daemon() -> (Daemon, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut daemon = Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    daemon.detect_settings_edit().unwrap();
    (daemon, dir)
}

fn edit(daemon: &Daemon, values: &[(&str, &str)]) -> SettingsSync {
    let mut wire = daemon.settings.state.as_ref().unwrap().to_proto();
    wire.set_at += 1;
    wire.values = values
        .iter()
        .map(|(key, value)| ((*key).into(), (*value).into()))
        .collect();
    wire
}

#[tokio::test]
async fn receive_settings_swapped_home_and_lock_accepts_snapshot() {
    // Arrange
    let (mut daemon, _dir) = daemon();
    daemon.config.set("hotkeys.home", "escape").unwrap();
    let home = daemon.config.get("hotkeys.home").unwrap();
    let lock = daemon.config.get("hotkeys.lock").unwrap();
    let wire = edit(&daemon, &[("hotkeys.home", &lock), ("hotkeys.lock", &home)]);

    // Act
    daemon.receive_settings(&wire).await.unwrap();

    // Assert
    assert_eq!(daemon.config.get("hotkeys.home"), Some(lock.clone()));
    assert_eq!(daemon.config.get("hotkeys.lock"), Some(home.clone()));
    let saved = Config::load(&daemon.config_path).unwrap();
    assert_eq!(saved.get("hotkeys.home"), Some(lock));
    assert_eq!(saved.get("hotkeys.lock"), Some(home));
    assert_eq!(
        daemon.settings.state,
        Some(Settings::try_from(&wire).unwrap())
    );
}

#[tokio::test]
async fn receive_settings_peer_security_key_keeps_local_policy() {
    // Arrange
    let (a, _a_dir) = daemon();
    let (mut b, b_dir) = daemon();
    let key = format!("peers.{}.allow_drive", a.identity.machine_id);
    b.config.set(&key, "true").unwrap();
    b.config.save(&b.config_path).unwrap();
    let wire = edit(
        &b,
        &[("input.speed", "2"), (&key, "false"), ("name", "hostile")],
    );

    // Act
    b.receive_settings(&wire).await.unwrap();

    // Assert
    let saved = Config::load(&b.config_path).unwrap();
    assert_eq!(saved.get(&key).as_deref(), Some("true"));
    assert_eq!(b.config.get(&key).as_deref(), Some("true"));
    assert!(saved.name.is_none());
    assert_eq!(saved.input.speed, 2.0);
    let restored = SettingsStore::load(&b_dir.path().join("settings.pb"));
    assert!(!restored.state.unwrap().values.contains_key(&key));
}

#[tokio::test]
async fn receive_settings_invalid_value_rejects_entire_message() {
    // Arrange
    let (mut daemon, _dir) = daemon();
    daemon.config.save(&daemon.config_path).unwrap();
    let before = std::fs::read(&daemon.config_path).unwrap();
    let snapshot = daemon.settings.state.clone();
    let wire = edit(
        &daemon,
        &[("input.natural_scrolling", "true"), ("input.speed", "0")],
    );

    // Act
    daemon.receive_settings(&wire).await.unwrap();

    // Assert
    assert_eq!(std::fs::read(&daemon.config_path).unwrap(), before);
    assert!(!daemon.config.input.natural_scrolling);
    assert_eq!(daemon.settings.state, snapshot);
}

#[tokio::test]
async fn receive_settings_old_or_disabled_message_keeps_config_and_snapshot() {
    for mode in ["older", "local_off", "remote_off"] {
        // Arrange
        let (mut daemon, _dir) = daemon();
        let mut wire = edit(&daemon, &[("input.speed", "2")]);
        match mode {
            "older" => wire.set_at = 0,
            "local_off" => daemon.config.sync.settings = false,
            _ => wire.on = false,
        }
        daemon.config.save(&daemon.config_path).unwrap();
        let before = std::fs::read(&daemon.config_path).unwrap();
        let snapshot = daemon.settings.state.clone();

        // Act
        daemon.receive_settings(&wire).await.unwrap();

        // Assert
        assert_eq!(std::fs::read(&daemon.config_path).unwrap(), before);
        assert_eq!(daemon.config.input.speed, 1.0);
        assert_eq!(daemon.settings.state, snapshot);
        if mode == "local_off" {
            assert!(daemon.settings_message().is_none());
        }
    }
}

#[tokio::test]
async fn reload_config_unchanged_remote_snapshot_preserves_version() {
    // Arrange
    let (mut daemon, dir) = daemon();
    let mut wire = daemon.settings.state.as_ref().unwrap().to_proto();
    wire.set_at += 1;
    wire.values.insert("input.speed".into(), "2.0".into());
    daemon.receive_settings(&wire).await.unwrap();
    let snapshot = daemon.settings.state.clone();

    // Act
    daemon.reload_config().await.unwrap();

    // Assert
    assert_eq!(daemon.settings.state, snapshot);
    let mut restarted = Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .unwrap()
        .with_config(
            Config::load(&daemon.config_path).unwrap(),
            daemon.config_path.clone(),
        );
    restarted.detect_settings_edit().unwrap();
    assert_eq!(restarted.settings.state, snapshot);
}

#[tokio::test]
async fn detect_settings_edit_clock_behind_snapshot_advances_timestamp() {
    // Arrange
    let (mut daemon, _dir) = daemon();
    let state = daemon.settings.state.as_mut().unwrap();
    state.set_at = u64::MAX - 1;
    daemon.config.input.speed = 2.0;

    // Act
    daemon.detect_settings_edit().unwrap();

    // Assert
    assert_eq!(daemon.settings.state.as_ref().unwrap().set_at, u64::MAX);
}

#[tokio::test]
async fn detect_settings_edit_exhausted_timestamp_still_records_edit() {
    // Arrange
    let (mut daemon, _dir) = daemon();
    let state = daemon.settings.state.as_mut().unwrap();
    state.set_at = u64::MAX;
    daemon.config.input.speed = 3.0;

    // Act
    let result = daemon.detect_settings_edit();

    // Assert
    assert!(result.is_ok());
    assert_eq!(
        daemon.settings.state.as_ref().unwrap().values["input.speed"],
        "3.0"
    );
}

#[test]
fn load_missing_corrupt_or_oversized_snapshot_starts_without_settings() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.pb");
    assert!(SettingsStore::load(&path).state.is_none());
    for bytes in [
        vec![255],
        vec![],
        vec![0; a7n_proto::limits::CONTROL_FRAME_MAX + 1],
    ] {
        std::fs::write(&path, bytes).unwrap();

        // Act
        let store = SettingsStore::load(&path);

        // Assert
        assert!(store.state.is_none());
    }
}

#[tokio::test]
async fn reload_config_sync_disabled_tracks_edits_without_sending() {
    // Arrange
    let (mut daemon, _dir) = daemon();
    daemon.config.sync.settings = false;
    daemon.config.input.speed = 2.0;
    daemon.config.save(&daemon.config_path).unwrap();
    let old = daemon.settings.state.clone().unwrap();

    // Act
    daemon.reload_config().await.unwrap();

    // Assert
    let edited = daemon.settings.state.clone().unwrap();
    assert!(edited.set_at > old.set_at);
    assert_eq!(edited.values["input.speed"], "2.0");
    assert!(daemon.settings_message().is_none());
    let mut config = daemon.config.clone();
    config.sync.settings = true;
    config.save(&daemon.config_path).unwrap();
    daemon.reload_config().await.unwrap();
    assert_eq!(daemon.settings.state, Some(edited));
    assert!(daemon.settings_message().is_some());
}
