// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_core::hotkey::{Bindings, Chord, DoubleTap, Mods};
use serde::{Deserialize, Serialize};

use super::{Config, ConfigError};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HotkeyConfig {
    pub double_tap: String,
    pub home: Vec<String>,
    pub lock: String,
    pub jump: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            double_tap: "right_alt".into(),
            home: vec!["scroll_lock".into(), "ctrl+alt+shift+home".into()],
            lock: "ctrl+alt+shift+l".into(),
            jump: "ctrl+alt+shift".into(),
        }
    }
}

impl Config {
    /// Build matcher bindings from a config already checked by `validate`.
    /// # Panics
    /// Panics if public hotkey fields were edited without validating them.
    #[must_use]
    pub fn bindings(&self) -> Bindings {
        self.parse_bindings()
            .expect("hotkey settings have been validated")
    }

    pub(super) fn parse_bindings(&self) -> Result<Bindings, ConfigError> {
        let invalid = |key: &str, error| ConfigError::Invalid(format!("hotkeys.{key}: {error}"));
        let bindings = Bindings {
            double_tap: DoubleTap::parse(&self.hotkeys.double_tap)
                .map_err(|error| invalid("double_tap", error))?,
            home: self
                .hotkeys
                .home
                .iter()
                .map(|text| Chord::parse(text).map_err(|error| invalid("home", error)))
                .collect::<Result<_, _>>()?,
            lock: Chord::parse(&self.hotkeys.lock).map_err(|error| invalid("lock", error))?,
            jump_mods: Mods::parse(&self.hotkeys.jump).map_err(|error| invalid("jump", error))?,
        };
        for (index, chord) in bindings
            .home
            .iter()
            .chain(std::iter::once(&bindings.lock))
            .enumerate()
        {
            let key = if index == bindings.home.len() {
                "lock"
            } else {
                "home"
            };
            if bindings.home[..index.min(bindings.home.len())].contains(chord)
                || chord.jump_target(bindings.jump_mods).is_some()
            {
                return Err(ConfigError::Invalid(format!(
                    "hotkeys.{key}: colliding chord"
                )));
            }
        }
        Ok(bindings)
    }
}
