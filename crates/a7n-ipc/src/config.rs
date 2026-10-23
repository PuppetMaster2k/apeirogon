// SPDX-License-Identifier: GPL-3.0-or-later

//! Local settings shared by the daemon and its clients.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use a7n_proto::{MachineId, limits};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

const MIN_SPEED: f64 = 0.1;
const MAX_SPEED: f64 = 10.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub port: u16,
    pub input: InputConfig,
    pub peers: BTreeMap<String, PeerConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            name: None,
            port: limits::DEFAULT_UDP_PORT,
            input: InputConfig::default(),
            peers: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InputConfig {
    pub speed: f64,
    pub natural_scrolling: bool,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            speed: 1.0,
            natural_scrolling: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PeerConfig {
    pub allow_drive: bool,
    pub modifier_remap: BTreeMap<Modifier, Modifier>,
}

impl Default for PeerConfig {
    fn default() -> Self {
        Self {
            allow_drive: true,
            modifier_remap: BTreeMap::new(),
        }
    }
}

/// Physical modifier names; enum values are their USB HID keyboard usages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", try_from = "String")]
#[repr(u32)]
pub enum Modifier {
    LeftCtrl = 0x0007_00e0,
    LeftShift,
    LeftAlt,
    LeftMeta,
    RightCtrl,
    RightShift,
    RightAlt,
    RightMeta,
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
    #[error("no home directory found for config.toml")]
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
        let mut ids = BTreeSet::new();
        for id in self.peers.keys() {
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
        let parts: Vec<_> = key.split('.').collect();
        let invalid = || ConfigError::Invalid(format!("{key} has an invalid value"));
        let value =
            match parts.as_slice() {
                ["name"] | ["peers", _, "modifier_remap", _] => toml::Value::String(value.into()),
                ["port"] => toml::Value::Integer(value.parse().map_err(|_| {
                    ConfigError::Invalid("port must be between 1 and 65535".into())
                })?),
                ["input", "speed"] => toml::Value::Float(value.parse().map_err(|_| {
                    ConfigError::Invalid("input.speed must be between 0.1 and 10.0".into())
                })?),
                ["input", "natural_scrolling"] | ["peers", _, "allow_drive"] => {
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
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    /// Read a dotted key as a scalar; absent keys return None.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<String> {
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
