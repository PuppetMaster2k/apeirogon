// SPDX-License-Identifier: GPL-3.0-or-later

//! Local cursor visibility while driving or after remote ownership ends.

#[derive(Debug)]
pub struct IdleCursor {
    enabled: bool,
    hidden: bool,
    reason: Reason,
}

#[derive(Debug, PartialEq, Eq)]
enum Reason {
    Home,
    Idle,
    Driving,
}

impl IdleCursor {
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            hidden: false,
            reason: Reason::Home,
        }
    }

    pub fn configure(&mut self, enabled: bool) -> Option<bool> {
        self.enabled = enabled;
        if !enabled && self.reason == Reason::Idle {
            self.reason = Reason::Home;
        }
        self.update()
    }

    pub fn ended_driven(&mut self) -> Option<bool> {
        if self.reason != Reason::Driving {
            self.reason = if self.enabled {
                Reason::Idle
            } else {
                Reason::Home
            };
        }
        self.update()
    }

    /// Focus returning home or becoming driven clears the previous hiding reasons.
    pub fn driving(&mut self, driving: bool) -> Option<bool> {
        self.reason = if driving {
            Reason::Driving
        } else {
            Reason::Home
        };
        self.update()
    }

    /// Motion clears idle hiding, but must not reveal a cursor while driving.
    pub fn show(&mut self) -> Option<bool> {
        if self.reason == Reason::Idle {
            self.reason = Reason::Home;
        }
        self.update()
    }

    fn update(&mut self) -> Option<bool> {
        let hidden = self.enabled && self.reason != Reason::Home;
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

    #[test]
    fn driving_motion_and_peer_handover_stays_hidden_until_home() {
        // Arrange
        let mut cursor = IdleCursor::new(true);

        // Act
        let changes = [
            cursor.driving(true),
            cursor.show(),
            cursor.driving(true),
            cursor.driving(false),
        ];

        // Assert
        assert_eq!(changes, [Some(true), None, None, Some(false)]);
    }

    #[test]
    fn driving_becomes_driven_then_released_hides_only_without_input() {
        // Arrange
        let mut cursor = IdleCursor::new(true);
        cursor.driving(true);

        // Act
        let changes = [cursor.driving(false), cursor.ended_driven(), cursor.show()];

        // Assert
        assert_eq!(changes, [Some(false), Some(true), Some(false)]);
    }

    #[test]
    fn driving_idle_reason_cleared_keeps_driving_hidden() {
        // Arrange
        let mut cursor = IdleCursor::new(true);
        cursor.driving(true);

        // Act
        let changes = [cursor.ended_driven(), cursor.show(), cursor.driving(false)];

        // Assert
        assert_eq!(changes, [None, None, Some(false)]);
    }

    #[test]
    fn configure_while_driving_restores_then_hides_when_enabled() {
        // Arrange
        let mut cursor = IdleCursor::new(true);
        cursor.driving(true);

        // Act
        let changes = [
            cursor.configure(false),
            cursor.show(),
            cursor.configure(true),
        ];

        // Assert
        assert_eq!(changes, [Some(false), None, Some(true)]);
    }
}
