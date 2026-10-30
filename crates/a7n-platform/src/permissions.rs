// SPDX-License-Identifier: GPL-3.0-or-later

//! macOS privacy permissions the daemon needs before it can share input.
//!
//! macOS grants these to the process that uses them, so they are checked and
//! requested here, in the daemon, and not in any GUI.

use std::fmt;

#[cfg(target_os = "macos")]
mod macos;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    Accessibility,
    InputMonitoring,
}

impl Permission {
    /// The label macOS shows in System Settings.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Accessibility => "Accessibility",
            Self::InputMonitoring => "Input Monitoring",
        }
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PermissionError {
    #[error("permissions are not managed on this platform")]
    Unsupported,
    #[error("cannot open the {0} pane in System Settings")]
    OpenSettings(Permission),
}

/// Permissions still missing, checked live on every call; always empty off macOS.
#[must_use]
pub fn missing() -> Vec<Permission> {
    #[cfg(target_os = "macos")]
    return macos::missing();
    #[cfg(not(target_os = "macos"))]
    Vec::new()
}

/// Registers the daemon for `permission` and opens its System Settings pane.
///
/// # Errors
///
/// [`PermissionError::Unsupported`] off macOS, or
/// [`PermissionError::OpenSettings`] when the pane cannot be opened.
pub fn request(permission: Permission) -> Result<(), PermissionError> {
    #[cfg(target_os = "macos")]
    return macos::request(permission);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = permission;
        Err(PermissionError::Unsupported)
    }
}
