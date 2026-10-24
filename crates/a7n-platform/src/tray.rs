// SPDX-License-Identifier: GPL-3.0-or-later

//! Native tray integration; Linux uses the regular settings window.
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
    pub fn update(&self, _status: &str, _focused: &str, _paused: bool) {}
    #[must_use]
    pub fn action(&self) -> Option<Action> {
        None
    }
}
