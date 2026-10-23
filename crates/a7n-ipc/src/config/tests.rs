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
