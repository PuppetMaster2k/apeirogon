// SPDX-License-Identifier: GPL-3.0-or-later

//! Native tray integration; Linux uses the regular settings window.
#[cfg(any(target_os = "windows", target_os = "macos"))]
mod icon;
#[cfg(target_os = "windows")]
mod menu_dismiss;
#[cfg(any(target_os = "windows", target_os = "macos"))]
mod native;
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub use native::Tray;

#[derive(Debug, Clone, Copy)]
pub enum Action {
    Pause,
    Settings,
    Quit,
}

/// What the tray glyph shows (docs/ui.md section 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Pointer and keyboard are on this machine.
    Idle,
    /// This machine drives another one.
    Remote,
    /// Input sharing is paused.
    Paused,
    /// Daemon unreachable or input unavailable.
    Error,
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
#[derive(Debug)]
pub struct Tray;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
impl Tray {
    /// Linux has no tray without additional desktop libraries.
    /// # Errors
    /// This fallback always returns an unsupported error.
    pub fn new(_wake: impl Fn() + Send + Sync + 'static) -> std::io::Result<Self> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "tray is available on Windows and macOS",
        ))
    }
    pub fn update(
        &self,
        _status: &str,
        _focused: &str,
        _pause: &str,
        _settings: &str,
        _quit: &str,
    ) {
    }
    /// # Errors
    /// Never fails; matches the native signature.
    pub fn set_state(&self, _state: State, _tooltip: &str) -> std::io::Result<()> {
        Ok(())
    }
    #[must_use]
    pub fn action(&self) -> Option<Action> {
        None
    }
}
