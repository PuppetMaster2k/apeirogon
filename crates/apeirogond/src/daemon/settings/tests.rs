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
async fn receive_settings_peer_security_key_keeps_local_policy() {
    // Arrange
    let (a, a_dir) = daemon();
    let (mut b, b_dir) = daemon();
    let clipboard_key = format!("peers.{}.clipboard", a.identity.machine_id);
    b.config.set(&clipboard_key, "off").unwrap();
    let files_key = format!("peers.{}.files", a.identity.machine_id);
    b.config.set(&files_key, "off").unwrap();
    let receive_dir = b_dir.path().join("received");
    b.config
        .set("files.receive_dir", receive_dir.to_str().unwrap())
        .unwrap();
    b.config.clipboard.deny_apps = vec!["secret.app".into()];
    let key = format!("peers.{}.allow_drive", a.identity.machine_id);
    b.config.set(&key, "true").unwrap();
    b.config.save(&b.config_path).unwrap();
    let wire = edit(
        &b,
        &[
            ("input.speed", "2"),
            (&key, "false"),
            (&clipboard_key, "both"),
            (&files_key, "both"),
            ("files.receive_dir", a_dir.path().to_str().unwrap()),
            ("clipboard.deny_apps", "[]"),
            ("name", "hostile"),
        ],
    );

    // Act
    b.receive_settings(&wire).await.unwrap();

    // Assert
    let saved = Config::load(&b.config_path).unwrap();
    assert_eq!(saved.get(&key).as_deref(), Some("true"));
    assert_eq!(b.config.get(&key).as_deref(), Some("true"));
    assert_eq!(saved.get(&clipboard_key).as_deref(), Some("off"));
    assert_eq!(b.config.get(&clipboard_key).as_deref(), Some("off"));
    assert_eq!(saved.get(&files_key).as_deref(), Some("off"));
    assert_eq!(b.config.get(&files_key).as_deref(), Some("off"));
    assert_eq!(saved.files.receive_dir.as_ref(), Some(&receive_dir));
    assert_eq!(b.config.files.receive_dir.as_ref(), Some(&receive_dir));
    assert_eq!(saved.clipboard.deny_apps, ["secret.app"]);
    assert!(saved.name.is_none());
    assert_eq!(saved.input.speed, 2.0);
    let restored = SettingsStore::load(&b_dir.path().join("settings.pb"));
    let values = restored.state.unwrap().values;
    assert!(!values.contains_key(&key));
    assert!(!values.contains_key(&clipboard_key));
    assert!(!values.contains_key(&files_key));
    assert!(!values.contains_key("files.receive_dir"));
    assert!(!values.contains_key("clipboard.deny_apps"));
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
async fn detect_settings_edit_file_settings_excludes_security_keys_from_message() {
    // Arrange
    let (mut daemon, dir) = daemon();
    let key = format!("peers.{}.files", daemon.identity.machine_id);
    daemon.config.set(&key, "both").unwrap();
    daemon
        .config
        .set("files.receive_dir", dir.path().to_str().unwrap())
        .unwrap();
    daemon.config.input.speed = 2.0;

    // Act
    daemon.detect_settings_edit().unwrap();

    // Assert
    let message = daemon.settings_message().unwrap();
    let Some(Body::Settings(wire)) = message.body else {
        panic!("expected settings message");
    };
    assert_eq!(wire.values["input.speed"], "2.0");
    assert!(!wire.values.contains_key(&key));
    assert!(!wire.values.contains_key("files.receive_dir"));
}
