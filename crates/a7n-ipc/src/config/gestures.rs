// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_core::{gestures::Bindings, hotkey::Chord};
use serde::{Deserialize, Serialize};

use super::{Config, ConfigError};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GestureConfig {
    pub swipe_up: String,
    pub swipe_down: String,
    pub swipe_left: String,
    pub swipe_right: String,
}

impl Default for GestureConfig {
    fn default() -> Self {
        Self {
            swipe_up: "meta+tab".into(),
            swipe_down: "meta+d".into(),
            swipe_left: "ctrl+meta+left".into(),
            swipe_right: "ctrl+meta+right".into(),
        }
    }
}

impl GestureConfig {
    /// Parse receiver-local shortcuts; an empty binding turns a swipe off.
    /// # Errors
    /// Names the gesture whose chord is invalid.
    pub fn bindings(&self) -> Result<Bindings, ConfigError> {
        let parse = |name, text: &str| {
            if text.is_empty() {
                return Ok(None);
            }
            Chord::parse(text)
                .map(Some)
                .map_err(|error| ConfigError::Invalid(format!("gestures.{name}: {error}")))
        };
        Ok(Bindings {
            swipe_up: parse("swipe_up", &self.swipe_up)?,
            swipe_down: parse("swipe_down", &self.swipe_down)?,
            swipe_left: parse("swipe_left", &self.swipe_left)?,
            swipe_right: parse("swipe_right", &self.swipe_right)?,
        })
    }
}

impl Config {
    /// Build gesture bindings from settings already checked by `validate`.
    /// # Panics
    /// Panics if public gesture fields were edited without validating them.
    #[must_use]
    pub fn gesture_bindings(&self) -> Bindings {
        self.gestures
            .bindings()
            .expect("gesture settings have been validated")
    }
}

#[cfg(test)]
mod tests {
    use super::super::Config;

    #[test]
    fn set_gesture_bindings_validates_and_preserves_on_error() {
        // Arrange
        let mut config = Config::default();
        for key in ["swipe_up", "swipe_down", "swipe_left", "swipe_right"] {
            let key = format!("gestures.{key}");
            config.set(&key, "").unwrap();
            assert_eq!(config.get(&key).as_deref(), Some(""));
            config.set(&key, "shift+up").unwrap();

            // Act
            let error = config.set(&key, "unknown").unwrap_err();

            // Assert
            assert!(error.to_string().starts_with(&format!("{key}:")));
            assert_eq!(config.get(&key).as_deref(), Some("shift+up"));
        }
        Config::parse(&config.to_toml().unwrap()).unwrap();
    }
}
