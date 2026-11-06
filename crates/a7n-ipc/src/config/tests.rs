// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_core::hotkey::Chord;

use super::*;

#[test]
fn parse_invalid_settings_rejects_unknown_keys_and_invalid_values() {
    // Arrange
    let cases = [
        ("unknown = true", "unknown"),
        ("port = 0", "port"),
        ("name = ''", "name"),
        ("[input]\nspeed = 0.09", "input.speed"),
        ("[input]\nspeed = 10.01", "input.speed"),
        ("[input]\nspeed = nan", "input.speed"),
        ("[input]\nspeed = inf", "input.speed"),
        ("[input]\nunknown = true", "unknown"),
        ("[peers.bad]\nallow_drive = true", "peers.bad"),
        (
            "[peers.0123456789abcdef0123456789abcdef]\nunknown = true",
            "unknown",
        ),
        (
            "[peers.0123456789abcdef0123456789abcdef.modifier_remap]\nleft_meta = 'caps_lock'",
            "modifier_remap",
        ),
        (
            "[peers.0123456789abcdef0123456789abcdef.modifier_remap]\ncaps_lock = 'left_ctrl'",
            "modifier_remap",
        ),
    ];
    for (text, key) in cases {
        // Act
        let error = Config::parse(text).unwrap_err();

        // Assert
        assert!(error.to_string().contains(key), "{text}: {error}");
    }
}

#[test]
fn parse_full_file_preserves_valid_settings() {
    // Arrange
    let text = r#"
name = "office-pc"
port = 12345
[input]
speed = 0.5
natural_scrolling = true
[peers.0123456789ABCDEF0123456789ABCDEF]
allow_drive = false
modifier_remap = { left_meta = "left_ctrl" }
"#;
    let machine = "0123456789abcdef0123456789abcdef".parse().unwrap();

    // Act
    let config = Config::parse(text).unwrap();

    // Assert
    assert_eq!(config.name.as_deref(), Some("office-pc"));
    assert_eq!(config.port, 12345);
    assert_eq!(config.input.speed, 0.5);
    assert!(config.input.natural_scrolling);
    let peer = config.peer(&machine).unwrap();
    assert!(!peer.allow_drive);
    assert_eq!(
        peer.modifier_remap[&Modifier::LeftMeta].usage(),
        0x0007_00e0
    );
}

#[test]
fn set_dotted_peer_key_round_trips_and_rejects_invalid_edit() {
    // Arrange
    let mut config = Config::default();
    let key = "peers.0123456789abcdef0123456789abcdef.modifier_remap.left_meta";
    config.set(key, "left_ctrl").unwrap();

    // Act
    let result = config.set(key, "caps_lock");

    // Assert
    assert!(result.is_err());
    let loaded = Config::parse(&config.to_toml().unwrap()).unwrap();
    assert_eq!(loaded.get(key).as_deref(), Some("left_ctrl"));
}

#[test]
fn init_existing_file_refuses_without_changing_contents() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let original = "# keep my config\nport = 12345\n";
    fs::write(&path, original).unwrap();

    // Act
    let error = Config::init(&path).unwrap_err();

    // Assert
    assert!(matches!(error, ConfigError::AlreadyExists(ref existing) if existing == &path));
    assert!(error.to_string().contains(&path.display().to_string()));
    assert_eq!(fs::read_to_string(path).unwrap(), original);
}

#[test]
fn load_missing_file_uses_defaults_and_rejects_invalid_existing_file() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let defaults = Config::load(&path).unwrap();
    fs::write(&path, "[input]\nspeed = 0").unwrap();

    // Act
    let result = Config::load(&path);

    // Assert
    assert_eq!(defaults.port, limits::DEFAULT_UDP_PORT);
    assert_eq!(defaults.input.speed, 1.0);
    assert!(result.is_err());
}

#[test]
fn parse_hotkey_collisions_and_bad_tokens_rejects_named_setting() {
    // Arrange
    for (settings, key) in [
        ("home = ['a', 'A']", "home"),
        ("home = ['ctrl+alt+shift+l']", "lock"),
        ("lock = 'ctrl+alt+shift+1'", "lock"),
        ("home = ['ctrl+alt+shift+9']", "home"),
        ("lock = 'foo'", "lock"),
        ("double_tap = 'left_home'", "double_tap"),
        ("jump = 'ctrl+1'", "jump"),
    ] {
        // Act
        let error = Config::parse(&format!("[hotkeys]\n{settings}")).unwrap_err();

        // Assert
        assert!(
            error.to_string().contains(&format!("hotkeys.{key}")),
            "{error}"
        );
    }
}

#[test]
fn set_hotkeys_round_trips_and_rejects_collision_without_mutation() {
    // Arrange
    let mut config = Config::default();
    for (key, value) in [
        ("double_tap", "none"),
        ("lock", "meta+l"),
        ("jump", "meta+shift"),
        ("home", "escape, ctrl+home"),
    ] {
        config.set(&format!("hotkeys.{key}"), value).unwrap();
    }
    let before = config.to_toml().unwrap();

    // Act
    let result = config.set("hotkeys.lock", "ctrl+home");

    // Assert
    assert!(result.is_err());
    assert_eq!(config.to_toml().unwrap(), before);
    let loaded = Config::parse(&before).unwrap();
    assert_eq!(
        loaded.get("hotkeys.home").as_deref(),
        Some("escape,ctrl+home")
    );
    assert_eq!(loaded.get("hotkeys.double_tap").as_deref(), Some("none"));
    assert_eq!(loaded.bindings().lock, Chord::parse("meta+l").unwrap());
}

#[test]
fn set_sync_settings_round_trips_and_rejects_non_boolean() {
    // Arrange
    let mut config = Config::parse("").unwrap();
    assert_eq!(config.get("sync.settings").as_deref(), Some("true"));
    config.set("sync.settings", "false").unwrap();

    // Act
    let result = config.set("sync.settings", "no");

    // Assert
    assert!(result.is_err());
    let loaded = Config::parse(&config.to_toml().unwrap()).unwrap();
    assert!(!loaded.sync.settings);
}

#[test]
fn set_file_notifications_round_trips_and_rejects_non_boolean() {
    // Arrange
    let mut config = Config::parse("").unwrap();
    assert!(config.notifications.files);
    config.set("notifications.files", "false").unwrap();

    // Act
    let result = config.set("notifications.files", "no");

    // Assert
    assert!(result.is_err());
    let loaded = Config::parse(&config.to_toml().unwrap()).unwrap();
    assert_eq!(loaded.get("notifications.files").as_deref(), Some("false"));
    assert!(!a7n_core::settings::SYNCED_KEYS.contains(&"notifications.files"));
}

#[test]
fn set_clipboard_policy_validates_values_and_keeps_deny_apps_file_only() {
    // Arrange
    let mut config = Config::parse("[clipboard]\ndeny_apps = ['secret.app']").unwrap();
    let key = "peers.01010101010101010101010101010101.clipboard";
    for policy in ["both", "send", "receive", "ask", "off"] {
        // Act
        config.set(key, policy).unwrap();
        let restored = Config::parse(&config.to_toml().unwrap()).unwrap();

        // Assert
        assert_eq!(restored.get(key).as_deref(), Some(policy));
        assert_eq!(restored.clipboard.deny_apps, ["secret.app"]);
    }
    assert!(config.set(key, "unknown").is_err());
    assert_eq!(config.get(key).as_deref(), Some("off"));
    assert!(config.set("clipboard.deny_apps", "[]").is_err());
}

#[test]
fn set_many_modifier_modes_round_trip_and_reject_invalid_value() {
    // Arrange
    let id = "0123456789abcdef0123456789abcdef";
    let key = format!("peers.{id}.modifier_mode");
    let mut config = Config::parse(&format!("[peers.{id}]\nallow_drive = true")).unwrap();
    assert_eq!(config.get(&key).as_deref(), Some("auto"));
    for mode in ["auto", "off", "custom"] {
        // Act
        config.set_many([(key.as_str(), mode)]).unwrap();
        let restored = Config::parse(&config.to_toml().unwrap()).unwrap();

        // Assert
        assert_eq!(restored.get(&key).as_deref(), Some(mode));
    }
    let before = config.to_toml().unwrap();

    // Act
    let result = config.set_many([(key.as_str(), "default")]);

    // Assert
    assert!(result.is_err());
    assert_eq!(config.to_toml().unwrap(), before);
    assert!(Config::parse(&format!("[peers.{id}]\nmodifier_mode = 'invalid'")).is_err());
}

#[test]
fn parse_peer_without_file_policy_defaults_to_ask() {
    // Arrange
    let id = "01010101010101010101010101010101";

    // Act
    let config = Config::parse(&format!("[peers.{id}]\nclipboard = 'off'")).unwrap();

    // Assert
    assert_eq!(
        config.get(&format!("peers.{id}.files")).as_deref(),
        Some("ask")
    );
}

#[test]
fn set_file_policy_round_trips_valid_values_and_rejects_invalid_value() {
    // Arrange
    let mut config = Config::default();
    let key = "peers.01010101010101010101010101010101.files";
    for policy in ["both", "send", "receive", "ask", "off"] {
        // Act
        config.set(key, policy).unwrap();
        let restored = Config::parse(&config.to_toml().unwrap()).unwrap();

        // Assert
        assert_eq!(restored.get(key).as_deref(), Some(policy));
        assert_eq!(
            restored.peers.values().next().unwrap().clipboard,
            ClipboardPolicy::Both
        );
    }

    // Act
    let result = config.set(key, "unknown");

    // Assert
    assert!(result.is_err());
    assert_eq!(config.get(key).as_deref(), Some("off"));
    assert!(Config::parse("[peers.01010101010101010101010101010101]\nfiles = 'unknown'").is_err());
}

#[test]
fn set_receive_dir_absolute_path_round_trips_without_creating_directory() {
    // Arrange
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("incoming files");
    let mut config = Config::default();

    // Act
    config
        .set("files.receive_dir", path.to_str().unwrap())
        .unwrap();
    let restored = Config::parse(&config.to_toml().unwrap()).unwrap();

    // Assert
    assert_eq!(restored.files.receive_dir.as_ref(), Some(&path));
    assert_eq!(
        restored.get("files.receive_dir"),
        Some(path.display().to_string())
    );
    assert!(!path.exists());
}

#[test]
fn set_receive_dir_relative_path_rejects_without_mutation() {
    // Arrange
    let mut config = Config::default();
    let before = config.to_toml().unwrap();
    for path in ["incoming", "../incoming", ""] {
        // Act
        let result = config.set("files.receive_dir", path);

        // Assert
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("files.receive_dir")
        );
        assert_eq!(config.to_toml().unwrap(), before);
        assert!(Config::parse(&format!("[files]\nreceive_dir = '{path}'")).is_err());
    }
}

#[test]
fn get_receive_dir_unset_returns_effective_download_directory() {
    // Arrange
    let config = Config::parse("").unwrap();
    let dirs = directories::UserDirs::new().unwrap();
    let expected = dirs
        .download_dir()
        .map_or_else(|| dirs.home_dir().join("Downloads"), Path::to_path_buf)
        .join("Apeirogon");

    // Act
    let value = config.get("files.receive_dir");

    // Assert
    assert_eq!(value, Some(expected.display().to_string()));
    assert!(config.files.receive_dir.is_none());
}

#[test]
fn set_trigger_settings_validates_keys_duration_and_collisions() {
    // Arrange
    for (key, value, valid) in [
        ("hotkeys.double_tap", "scroll_lock", false),
        ("hotkeys.hold_ms", "299", false),
        ("hotkeys.hold_ms", "3001", false),
        ("hotkeys.hold_ms", "-1", false),
        ("hotkeys.trigger_mode", "repeat", false),
        ("hotkeys.hold_ms", "300", true),
        ("hotkeys.hold_ms", "3000", true),
        ("hotkeys.trigger_mode", "hold", true),
        ("hotkeys.trigger_mode", "both", true),
        ("hotkeys.double_tap", "f13", true),
    ] {
        let mut config = Config::default();

        // Act
        let result = config.set(key, value);

        // Assert
        assert_eq!(result.is_ok(), valid, "{key} = {value}");
        if !valid {
            assert!(matches!(result, Err(ConfigError::Invalid(_))));
        }
        if value == "scroll_lock" {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("colliding trigger")
            );
        }
    }
}

#[test]
fn set_shake_count_accepts_only_off_or_safe_range() {
    // Arrange
    for count in [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11] {
        let mut config = Config::default();
        let value = count.to_string();

        // Act
        let result = config.set("hotkeys.shake", &value);

        // Assert
        assert_eq!(
            result.is_ok(),
            count == 0 || (3..=10).contains(&count),
            "{count}"
        );
        if result.is_ok() {
            assert_eq!(config.get("hotkeys.shake"), Some(value));
        }
    }
}
