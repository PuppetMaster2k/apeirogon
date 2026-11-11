// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_core::local_input::LocalInput;

use super::{Config, ConfigError};

impl Config {
    /// Resolve local physical input names without applying peer modifier remaps.
    /// # Errors
    /// Names the invalid local key or mouse button.
    pub fn local_inputs(&self) -> Result<Vec<LocalInput>, ConfigError> {
        self.input
            .local_keys
            .iter()
            .map(|name| {
                LocalInput::parse(name).map_err(|error| {
                    ConfigError::Invalid(format!("input.local_keys: '{name}': {error}"))
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_local_keys_validates_names_and_preserves_config_on_error() {
        // Arrange
        let mut config = Config::default();
        config.set("input.local_keys", "back,volume_up").unwrap();
        let invalid = ["left", "right", "unknown", "back,,middle"];

        for name in invalid {
            // Act
            let result = config.set("input.local_keys", name);

            // Assert
            assert!(result.unwrap_err().to_string().contains("input.local_keys"));
            assert_eq!(config.get("input.local_keys").unwrap(), "back,volume_up");
            assert!(Config::parse(&format!("[input]\nlocal_keys = ['{name}']")).is_err());
        }
    }

    #[test]
    fn set_local_keys_empty_value_clears_list() {
        // Arrange
        let mut config = Config::default();
        config.set("input.local_keys", "back,volume_up").unwrap();

        // Act
        config.set("input.local_keys", "").unwrap();

        // Assert
        assert_eq!(config.local_inputs().unwrap(), Vec::<LocalInput>::new());
        assert!(!a7n_core::settings::SYNCED_KEYS.contains(&"input.local_keys"));
    }
}
