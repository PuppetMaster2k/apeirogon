// SPDX-License-Identifier: GPL-3.0-or-later

//! Local settings shared by the daemon and its clients.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use a7n_core::modifiers::{LEFT_CTRL, LEFT_META, RIGHT_CTRL, RIGHT_META};
use a7n_proto::{MachineId, limits};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

mod files;
mod gestures;
mod hotkeys;
mod local_keys;
pub use files::FilesConfig;
pub use gestures::GestureConfig;
pub use hotkeys::HotkeyConfig;

pub const MIN_SPEED: f64 = 0.1;
pub const MAX_SPEED: f64 = 10.0;
pub const MIN_PEER_SPEED: f64 = 0.25;
pub const MAX_PEER_SPEED: f64 = 4.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub port: u16,
    pub input: InputConfig,
    pub feedback: FeedbackConfig,
    pub hotkeys: HotkeyConfig,
    pub gestures: GestureConfig,
    pub sync: SyncConfig,
    pub peers: BTreeMap<String, PeerConfig>,
    pub clipboard: ClipboardConfig,
    pub files: FilesConfig,
    pub notifications: NotificationsConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            name: None,
            port: limits::DEFAULT_UDP_PORT,
            input: InputConfig::default(),
            feedback: FeedbackConfig::default(),
            hotkeys: HotkeyConfig::default(),
            gestures: GestureConfig::default(),
            sync: SyncConfig::default(),
            peers: BTreeMap::new(),
            clipboard: ClipboardConfig::default(),
            files: FilesConfig::default(),
            notifications: NotificationsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NotificationsConfig {
    pub files: bool,
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self { files: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SyncConfig {
    pub settings: bool,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self { settings: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InputConfig {
    pub speed: f64,
    pub natural_scrolling: bool,
    pub fullscreen_hold: bool,
    pub local_keys: Vec<String>,
    pub hide_idle_cursor: bool,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            speed: 1.0,
            natural_scrolling: false,
            fullscreen_hold: false,
            local_keys: Vec::new(),
            hide_idle_cursor: true,
        }
    }
}

/// Local cues for edge pushes and landings; never synced between machines.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FeedbackConfig {
    pub edge: bool,
    pub landing: bool,
    pub haptics: bool,
}

impl Default for FeedbackConfig {
    fn default() -> Self {
        Self {
            edge: true,
            landing: true,
            haptics: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PeerConfig {
    pub allow_drive: bool,
    pub wake: bool,
    pub pointer_speed: f64,
    pub scroll_speed: f64,
    pub reverse_scroll: bool,
    pub clipboard: ClipboardPolicy,
    pub files: ClipboardPolicy,
    pub modifier_mode: ModifierMode,
    pub modifier_remap: BTreeMap<Modifier, Modifier>,
}

impl Default for PeerConfig {
    fn default() -> Self {
        Self {
            allow_drive: true,
            wake: true,
            pointer_speed: 1.0,
            scroll_speed: 1.0,
            reverse_scroll: false,
            clipboard: ClipboardPolicy::default(),
            files: ClipboardPolicy::Ask,
            modifier_mode: ModifierMode::default(),
            modifier_remap: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClipboardConfig {
    pub deny_apps: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardPolicy {
    #[default]
    Both,
    Send,
    Receive,
    Ask,
    Off,
}

impl ClipboardPolicy {
    /// Whether this machine offers its copies to the peer (`ask` only gates receiving).
    #[must_use]
    pub fn sends(self) -> bool {
        matches!(self, Self::Both | Self::Send | Self::Ask)
    }

    /// Whether the peer's copies may reach this machine, after confirmation for `ask`.
    #[must_use]
    pub fn receives(self) -> bool {
        matches!(self, Self::Both | Self::Receive | Self::Ask)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModifierMode {
    #[default]
    Auto,
    Off,
    Custom,
}

/// Physical modifier names; enum values are their USB HID keyboard usages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", try_from = "String")]
#[repr(u32)]
pub enum Modifier {
    LeftCtrl = LEFT_CTRL,
    LeftShift,
    LeftAlt,
    LeftMeta = LEFT_META,
    RightCtrl = RIGHT_CTRL,
    RightShift,
    RightAlt,
    RightMeta = RIGHT_META,
}

impl Modifier {
    #[must_use]
    pub const fn usage(self) -> u32 {
        self as u32
    }
}

impl TryFrom<String> for Modifier {
    type Error = &'static str;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        match name.as_str() {
            "left_ctrl" => Ok(Self::LeftCtrl),
            "left_shift" => Ok(Self::LeftShift),
            "left_alt" => Ok(Self::LeftAlt),
            "left_meta" => Ok(Self::LeftMeta),
            "right_ctrl" => Ok(Self::RightCtrl),
            "right_shift" => Ok(Self::RightShift),
            "right_alt" => Ok(Self::RightAlt),
            "right_meta" => Ok(Self::RightMeta),
            _ => Err(
                "modifier_remap names must be left_ctrl, left_shift, left_alt, left_meta, right_ctrl, right_shift, right_alt or right_meta",
            ),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("no home directory found")]
    NoHome,
    #[error("config already exists: {0}")]
    AlreadyExists(PathBuf),
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error(transparent)]
    Parse(#[from] toml::de::Error),
    #[error(transparent)]
    Serialize(#[from] toml::ser::Error),
    #[error("{0}")]
    Invalid(String),
}

impl Config {
    /// Locate the current user's config without creating it.
    /// # Errors
    /// Returns an error if the home directory is unavailable.
    pub fn path() -> Result<PathBuf, ConfigError> {
        Ok(ProjectDirs::from("", "", "apeirogon")
            .ok_or(ConfigError::NoHome)?
            .config_dir()
            .join("config.toml"))
    }

    /// Missing files use defaults; other read errors are reported.
    /// # Errors
    /// Returns file, TOML syntax or validation errors.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Io {
                path: path.into(),
                source,
            }),
        }
    }

    /// Parse and validate all settings, rejecting unknown fields.
    /// # Errors
    /// Returns TOML syntax or validation errors.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(text)?;
        config.validate()?;
        Ok(config)
    }

    /// Validate settings before they are persisted or used.
    /// # Errors
    /// Names the invalid key and its rule.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self
            .name
            .as_ref()
            .is_some_and(|name| !(1..=limits::PEER_NAME_MAX).contains(&name.chars().count()))
        {
            return Err(ConfigError::Invalid(format!(
                "name must contain 1 to {} code points",
                limits::PEER_NAME_MAX
            )));
        }
        if self.port == 0 {
            return Err(ConfigError::Invalid(
                "port must be between 1 and 65535".into(),
            ));
        }
        if !(MIN_SPEED..=MAX_SPEED).contains(&self.input.speed) {
            return Err(ConfigError::Invalid(
                "input.speed must be between 0.1 and 10.0".into(),
            ));
        }
        self.local_inputs()?;
        self.parse_bindings()?;
        self.gestures.bindings()?;
        if self
            .files
            .receive_dir
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
        {
            return Err(ConfigError::Invalid(
                "files.receive_dir must be absolute".into(),
            ));
        }
        let mut ids = BTreeSet::new();
        for (id, peer) in &self.peers {
            for (field, speed) in [
                ("pointer_speed", peer.pointer_speed),
                ("scroll_speed", peer.scroll_speed),
            ] {
                if !(MIN_PEER_SPEED..=MAX_PEER_SPEED).contains(&speed) {
                    return Err(ConfigError::Invalid(format!(
                        "peers.{id}.{field} must be between {MIN_PEER_SPEED} and {MAX_PEER_SPEED}"
                    )));
                }
            }
            let machine = id.parse::<MachineId>().map_err(|_| {
                ConfigError::Invalid(format!("peers.{id} must be a 32 hex machine id"))
            })?;
            if !ids.insert(machine) {
                return Err(ConfigError::Invalid(format!(
                    "peers.{id} duplicates a machine id"
                )));
            }
        }
        Ok(())
    }

    /// Resolve either upper- or lower-case hex table names.
    #[must_use]
    pub fn peer(&self, machine: &MachineId) -> Option<&PeerConfig> {
        self.peers.iter().find_map(|(id, peer)| {
            (id.parse::<MachineId>().ok().as_ref() == Some(machine)).then_some(peer)
        })
    }

    /// Set one supported dotted key. Invalid edits leave the config unchanged.
    /// # Errors
    /// Rejects unsupported keys, invalid values and invalid resulting settings.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), ConfigError> {
        self.set_many([(key, value)])
    }

    /// Apply supported dotted keys together, leaving the config unchanged on error.
    /// # Errors
    /// Rejects unsupported keys, invalid values and invalid final settings.
    pub fn set_many<'a>(
        &mut self,
        values: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<(), ConfigError> {
        let mut candidate = self.clone();
        for (key, value) in values {
            candidate.set_unchecked(key, value)?;
        }
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    fn set_unchecked(&mut self, key: &str, value: &str) -> Result<(), ConfigError> {
        let parts: Vec<_> = key.split('.').collect();
        let invalid = || ConfigError::Invalid(format!("{key} has an invalid value"));
        let value =
            match parts.as_slice() {
                ["name"]
                | ["files", "receive_dir"]
                | ["peers", _, "clipboard" | "files" | "modifier_mode"]
                | ["peers", _, "modifier_remap", _]
                | [
                    "gestures",
                    "swipe_up" | "swipe_down" | "swipe_left" | "swipe_right",
                ]
                | ["hotkeys", "double_tap" | "trigger_mode" | "lock" | "jump"] => {
                    toml::Value::String(value.into())
                }
                ["hotkeys", "home"] | ["input", "local_keys"] => toml::Value::Array(
                    value
                        .split(',')
                        .filter(|entry| !value.is_empty() || !entry.is_empty())
                        .map(|chord| toml::Value::String(chord.trim().into()))
                        .collect(),
                ),
                ["hotkeys", "hold_ms" | "shake"] => toml::Value::Integer(
                    value.parse::<u32>().map(i64::from).map_err(|_| invalid())?,
                ),
                ["port"] => toml::Value::Integer(value.parse().map_err(|_| {
                    ConfigError::Invalid("port must be between 1 and 65535".into())
                })?),
                ["peers", _, "pointer_speed" | "scroll_speed"] => {
                    toml::Value::Float(value.parse().map_err(|_| invalid())?)
                }
                ["input", "speed"] => toml::Value::Float(value.parse().map_err(|_| {
                    ConfigError::Invalid("input.speed must be between 0.1 and 10.0".into())
                })?),
                [
                    "input",
                    "natural_scrolling" | "fullscreen_hold" | "hide_idle_cursor",
                ]
                | ["feedback", "edge" | "landing" | "haptics"]
                | ["notifications", "files"]
                | ["peers", _, "allow_drive" | "wake" | "reverse_scroll"]
                | ["sync", "settings"] => {
                    toml::Value::Boolean(value.parse().map_err(|_| {
                        ConfigError::Invalid(format!("{key} must be true or false"))
                    })?)
                }
                _ => {
                    return Err(ConfigError::Invalid(format!(
                        "{key} is not a supported config key"
                    )));
                }
            };
        let mut table = toml::Value::try_from(&*self)?;
        let mut node = &mut table;
        for part in &parts[..parts.len() - 1] {
            node = node
                .as_table_mut()
                .ok_or_else(invalid)?
                .entry((*part).to_owned())
                .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        }
        node.as_table_mut()
            .ok_or_else(invalid)?
            .insert(parts[parts.len() - 1].into(), value);
        let candidate: Self = table
            .try_into()
            .map_err(|error| ConfigError::Invalid(format!("{key}: {error}")))?;
        *self = candidate;
        Ok(())
    }

    /// Read a dotted key as a scalar, resolving the default receive directory.
    /// Unknown or unavailable keys return None.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<String> {
        if key == "files.receive_dir" {
            return self
                .files
                .receive_dir()
                .ok()
                .map(|path| path.display().to_string());
        }
        if key == "input.local_keys" {
            return Some(self.input.local_keys.join(","));
        }
        if key == "hotkeys.home" {
            return Some(self.hotkeys.home.join(","));
        }
        let mut value = toml::Value::try_from(self).ok()?;
        for part in key.split('.') {
            value = value.as_table_mut()?.remove(part)?;
        }
        Some(match value {
            toml::Value::String(text) => text,
            value => value.to_string(),
        })
    }

    /// Serialize settings. Existing comments and formatting are not retained.
    /// # Errors
    /// Returns validation or serialization errors.
    pub fn to_toml(&self) -> Result<String, ConfigError> {
        self.validate()?;
        Ok(toml::to_string_pretty(self)?)
    }

    /// Atomically save a user-only config, creating the parent directory.
    /// # Errors
    /// Returns validation, serialization or file errors.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let text = self.to_toml()?;
        let write = || {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                fs::create_dir_all(parent)?;
            }
            a7n_platform::fs::write_user_only(path, text.as_bytes())
        };
        write().map_err(|source| ConfigError::Io {
            path: path.into(),
            source,
        })
    }

    /// Create initial settings with the OS host name, refusing existing files.
    /// # Errors
    /// Returns an error with the path if it exists, or if writing fails.
    pub fn init(path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|source| ConfigError::Io {
                path: path.into(),
                source,
            })?;
        }
        // Reserve the name so concurrent init calls cannot overwrite a config.
        match fs::File::create_new(path) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(ConfigError::AlreadyExists(path.into()));
            }
            Err(source) => {
                return Err(ConfigError::Io {
                    path: path.into(),
                    source,
                });
            }
        }
        let result = Self {
            name: a7n_platform::host_name(),
            ..Self::default()
        }
        .save(path);
        if result.is_err() {
            let _ = fs::remove_file(path);
        }
        result
    }
}

#[cfg(test)]
mod tests;
