// SPDX-License-Identifier: GPL-3.0-or-later

//! Local cursor visibility after remote ownership ends.

#[derive(Debug)]
pub struct IdleCursor {
    enabled: bool,
    hidden: bool,
}

impl IdleCursor {
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            hidden: false,
        }
    }

    pub fn configure(&mut self, enabled: bool) -> Option<bool> {
        self.enabled = enabled;
        if enabled { None } else { self.show() }
    }

    pub fn ended_driven(&mut self) -> Option<bool> {
        self.set_hidden(self.enabled)
    }

    /// Local motion, remote motion, or a new driven session restores visibility.
    pub fn show(&mut self) -> Option<bool> {
        self.set_hidden(false)
    }

    fn set_hidden(&mut self, hidden: bool) -> Option<bool> {
        if self.hidden == hidden {
            return None;
        }
        self.hidden = hidden;
        Some(hidden)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ended_driven_enabled_hides_once() {
        // Arrange
        let mut cursor = IdleCursor::new(true);

        // Act
        let changes = [cursor.ended_driven(), cursor.ended_driven()];

        // Assert
        assert_eq!(changes, [Some(true), None]);
    }

    #[test]
    fn show_motion_or_new_driver_restores_cursor() {
        // Arrange
        let mut cursor = IdleCursor::new(true);
        cursor.ended_driven();

        // Act
        let changes = [cursor.show(), cursor.show()];

        // Assert
        assert_eq!(changes, [Some(false), None]);
    }

    #[test]
    fn configure_disabled_restores_and_never_hides() {
        // Arrange
        let mut cursor = IdleCursor::new(true);
        cursor.ended_driven();

        // Act
        let changes = [cursor.configure(false), cursor.ended_driven()];

        // Assert
        assert_eq!(changes, [Some(false), None]);
    }
}
