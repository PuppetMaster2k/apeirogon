// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

use super::ConfigError;

/// Cues for edge pushes and landings; appearance syncs, enable toggles stay local.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FeedbackConfig {
    pub edge: bool,
    pub landing: bool,
    pub haptics: bool,
    #[serde(flatten)]
    pub look: a7n_platform::feedback::Look,
}

impl Default for FeedbackConfig {
    fn default() -> Self {
        Self {
            edge: true,
            landing: true,
            haptics: false,
            look: a7n_platform::feedback::Look::default(),
        }
    }
}

impl FeedbackConfig {
    pub(super) fn validate(&self) -> Result<(), ConfigError> {
        let look = self.look;
        for (parameter, value) in a7n_platform::feedback::parameters::ALL.into_iter().zip([
            look.depth,
            look.radius,
            look.speed,
            look.frequency,
            look.intensity,
        ]) {
            if !(parameter.min..=parameter.max).contains(&value) {
                return Err(ConfigError::Invalid(format!(
                    "{} must be between {} and {}",
                    parameter.key, parameter.min, parameter.max
                )));
            }
        }
        Ok(())
    }
}
