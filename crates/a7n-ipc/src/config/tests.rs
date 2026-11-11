// SPDX-License-Identifier: GPL-3.0-or-later

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
        (
            "[peers.0123456789abcdef0123456789abcdef]\nfiles = 'unknown'",
            "files",
        ),
        (
            "[peers.0123456789abcdef0123456789abcdef]\nmodifier_mode = 'invalid'",
            "modifier_mode",
        ),
        ("[files]\nreceive_dir = 'incoming'", "receive_dir"),
    ];
    for (text, key) in cases {
        // Act
        let error = Config::parse(text).unwrap_err();

        // Assert
        assert!(error.to_string().contains(key), "{text}: {error}");
    }
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
fn set_hotkey_trigger_values_outside_allowed_range_are_rejected() {
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
        ("hotkeys.shake", "0", true),
        ("hotkeys.shake", "2", false),
        ("hotkeys.shake", "3", true),
        ("hotkeys.shake", "10", true),
        ("hotkeys.shake", "11", false),
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
fn set_invalid_value_leaves_config_unchanged() {
    // Arrange
    let id = "0123456789abcdef0123456789abcdef";
    let mut config = Config::parse(&format!(
        "[hotkeys]\nhome = ['escape', 'ctrl+home']\n[peers.{id}]\nclipboard = 'off'"
    ))
    .unwrap();
    let before = config.to_toml().unwrap();
    let cases = [
        ("hotkeys.lock", "ctrl+home"),
        ("sync.settings", "no"),
        ("notifications.files", "no"),
        ("input.fullscreen_hold", "yes"),
        ("input.hide_idle_cursor", "yes"),
        ("feedback.haptics", "yes"),
        ("files.receive_dir", "incoming"),
        ("files.receive_dir", "../incoming"),
        ("files.receive_dir", ""),
        ("gestures.swipe_up", "unknown"),
        // Clipboard exclusions are edited in the file only.
        ("clipboard.deny_apps", "[]"),
    ];
    let peer_cases = [
        ("clipboard", "unknown"),
        ("files", "unknown"),
        ("modifier_mode", "default"),
        ("modifier_remap.left_meta", "caps_lock"),
    ];
    let peer_cases = peer_cases.map(|(field, value)| (format!("peers.{id}.{field}"), value));

    for (key, value) in cases
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .chain(peer_cases)
    {
        // Act
        let result = config.set(&key, value);

        // Assert
        assert!(result.is_err(), "{key} = {value}");
        assert_eq!(config.to_toml().unwrap(), before, "{key} = {value}");
    }
}
