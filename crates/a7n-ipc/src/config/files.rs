// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

use directories::UserDirs;
use serde::{Deserialize, Serialize};

use super::ConfigError;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FilesConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receive_dir: Option<PathBuf>,
}

impl FilesConfig {
    /// Resolve the receive directory without creating it.
    /// # Errors
    /// Returns an error if neither an explicit directory nor a home is available.
    pub fn receive_dir(&self) -> Result<PathBuf, ConfigError> {
        if let Some(path) = &self.receive_dir {
            return Ok(path.clone());
        }
        let dirs = UserDirs::new().ok_or(ConfigError::NoHome)?;
        Ok(dirs.download_dir().map_or_else(
            || dirs.home_dir().join("Downloads").join("Apeirogon"),
            |path| path.join("Apeirogon"),
        ))
    }
}
