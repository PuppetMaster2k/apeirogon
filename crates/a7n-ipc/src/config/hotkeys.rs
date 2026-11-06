// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Duration;

use a7n_core::hotkey::{
    Bindings, Chord, DoubleTap, HOLD_DEFAULT, HOLD_MAX, HOLD_MIN, Mods, SHAKE_DEFAULT, SHAKE_MAX,
    SHAKE_MIN, TriggerMode,
};
use serde::{Deserialize, Serialize};

use super::{Config, ConfigError};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HotkeyConfig {
    pub double_tap: String,
    pub trigger_mode: String,
    pub hold_ms: u32,
    pub shake: u8,
    pub home: Vec<String>,
    pub lock: String,
    pub jump: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            double_tap: "right_alt".into(),
            trigger_mode: "double_tap".into(),
            hold_ms: HOLD_DEFAULT,
            shake: SHAKE_DEFAULT,
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
        if !(HOLD_MIN..=HOLD_MAX).contains(&self.hotkeys.hold_ms) {
            return Err(ConfigError::Invalid(format!(
                "hotkeys.hold_ms must be between {HOLD_MIN} and {HOLD_MAX}"
            )));
        }
        if self.hotkeys.shake != 0 && !(SHAKE_MIN..=SHAKE_MAX).contains(&self.hotkeys.shake) {
            return Err(ConfigError::Invalid(format!(
                "hotkeys.shake must be 0 or between {SHAKE_MIN} and {SHAKE_MAX}"
            )));
        }
        let bindings = Bindings {
            trigger_mode: TriggerMode::parse(&self.hotkeys.trigger_mode)
                .map_err(|error| invalid("trigger_mode", error))?,
            hold: Duration::from_millis(u64::from(self.hotkeys.hold_ms)),
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
            if bindings.double_tap.collides(*chord) {
                return Err(ConfigError::Invalid(format!(
                    "hotkeys.{key}: colliding trigger"
                )));
            }
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
