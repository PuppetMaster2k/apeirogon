// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_proto::{
    limits,
    v1::{Button, ButtonEvent, Edge},
};

/// Mouse buttons held for the current focus; updating it never allocates.
#[derive(Debug, Default)]
pub struct HeldButtons {
    pressed: u32,
}

impl HeldButtons {
    pub fn apply(&mut self, event: &ButtonEvent) {
        let Ok(button) = Button::try_from(event.button) else {
            return;
        };
        if button == Button::Unspecified {
            return;
        }
        let mask = 1 << i32::from(button);
        if event.down {
            self.pressed |= mask;
        } else {
            self.pressed &= !mask;
        }
    }

    #[must_use]
    pub fn any_held(&self) -> bool {
        self.pressed != 0
    }

    pub fn reset(&mut self) {
        self.pressed = 0;
    }
}

/// Accumulated overshoot on one edge; moving away or crossing resets it.
#[derive(Debug)]
pub struct EdgeResistance {
    resistance_px: u32,
    edge: Edge,
    overshoot_px: u32,
}

impl EdgeResistance {
    #[must_use]
    pub fn new(resistance_px: u32) -> Self {
        Self {
            resistance_px: if resistance_px == 0 {
                limits::DEFAULT_RESISTANCE_PX
            } else {
                resistance_px.min(limits::RESISTANCE_MAX_PX)
            },
            edge: Edge::Unspecified,
            overshoot_px: 0,
        }
    }

    /// Adds outward motion. Nonpositive motion or an unspecified edge resets it.
    pub fn push(&mut self, edge: Edge, overshoot_px: i32) -> bool {
        if edge != self.edge || overshoot_px <= 0 || edge == Edge::Unspecified {
            self.edge = edge;
            self.overshoot_px = 0;
        }
        let Ok(overshoot_px) = u32::try_from(overshoot_px) else {
            return false;
        };
        if edge == Edge::Unspecified {
            return false;
        }
        self.overshoot_px = self.overshoot_px.saturating_add(overshoot_px);
        if self.overshoot_px < self.resistance_px {
            return false;
        }
        self.overshoot_px = 0;
        true
    }
}

/// A focused full-screen window holds one continuous edge push.
#[derive(Debug)]
pub struct FullscreenHold {
    enabled: bool,
    held: Option<bool>,
}

impl FullscreenHold {
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            held: None,
        }
    }

    /// Clears the current push without changing whether holding is enabled.
    pub fn reset(&mut self) {
        self.held = None;
    }

    /// Queries only at the start of a push; moving away clears the result.
    pub fn holds(&mut self, pushing: bool, query: impl FnOnce() -> bool) -> bool {
        if !pushing || !self.enabled {
            self.reset();
            return false;
        }
        *self.held.get_or_insert_with(query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_disabled_or_ended_push_never_queries() {
        // Arrange
        let mut disabled = FullscreenHold::new(false);
        let mut ended = FullscreenHold::new(true);
        ended.holds(true, || true);

        // Act
        let disabled_result = disabled.holds(true, || panic!("disabled hold must not query"));
        let ended_result = ended.holds(false, || panic!("ending push must not query"));
        let next_push = ended.holds(true, || false);

        // Assert
        assert!(!disabled_result);
        assert!(!ended_result);
        assert!(!next_push);
    }

    #[test]
    fn holds_continuous_push_queries_once_and_preserves_result() {
        // Arrange
        for fullscreen in [false, true] {
            let mut hold = FullscreenHold::new(true);

            // Act
            let first = hold.holds(true, || fullscreen);
            let next = hold.holds(true, || panic!("continuous push must use cache"));

            // Assert
            assert_eq!((first, next), (fullscreen, fullscreen));
        }
    }

    #[test]
    fn push_button_held_blocks_crossing_and_discards_overshoot() {
        // Arrange
        let mut resistance = EdgeResistance::new(10);
        let mut buttons = HeldButtons::default();
        let button = |down| ButtonEvent {
            button: Button::Left.into(),
            down,
        };
        resistance.push(Edge::Right, 9);
        buttons.apply(&button(true));

        // Act
        let crossed = [(20, None), (9, Some(false)), (1, None)].map(|(px, down)| {
            if let Some(down) = down {
                buttons.apply(&button(down));
            }
            if buttons.any_held() {
                return resistance.push(Edge::Unspecified, 0);
            }
            resistance.push(Edge::Right, px)
        });

        // Assert
        assert_eq!(crossed, [false, false, true]);
    }

    #[test]
    fn apply_multiple_and_unknown_buttons_preserves_known_held_state() {
        // Arrange
        let mut buttons = HeldButtons::default();
        let events = [
            (Button::Left.into(), true),
            (Button::Forward.into(), true),
            (Button::Left.into(), false),
            (Button::Unspecified.into(), true),
            (-1, true),
            (i32::MAX, true),
            (Button::Forward.into(), false),
        ];

        // Act
        let held = events.map(|(button, down)| {
            buttons.apply(&ButtonEvent { button, down });
            buttons.any_held()
        });

        // Assert
        assert_eq!(held, [true, true, true, true, true, true, false]);
    }

    #[test]
    fn push_accumulated_overshoot_crosses_at_threshold_and_resets() {
        // Arrange
        let mut resistance = EdgeResistance::new(10);

        // Act
        let crossed = [3, 6, 1, 1].map(|px| resistance.push(Edge::Right, px));

        // Assert
        assert_eq!(crossed, [false, false, true, false]);
    }

    #[test]
    fn push_edge_change_or_moving_away_discards_previous_overshoot() {
        // Arrange
        let reset_cases = [
            (Edge::Right, 0),
            (Edge::Right, -1),
            (Edge::Unspecified, 5),
            (Edge::Left, 1),
        ];

        for (edge, overshoot) in reset_cases {
            let mut resistance = EdgeResistance::new(10);
            resistance.push(Edge::Right, 9);

            // Act
            let reset_crossed = resistance.push(edge, overshoot);
            let crossed = resistance.push(Edge::Right, 1);

            // Assert
            assert!(!reset_crossed);
            assert!(!crossed);
        }
    }

    #[test]
    fn push_absent_or_excessive_resistance_uses_default_or_maximum() {
        // Arrange
        let cases = [
            (0, limits::DEFAULT_RESISTANCE_PX),
            (u32::MAX, limits::RESISTANCE_MAX_PX),
        ];

        for (configured, threshold) in cases {
            let mut resistance = EdgeResistance::new(configured);
            let threshold = i32::try_from(threshold).expect("resistance limit fits i32");

            // Act
            let early = resistance.push(Edge::Bottom, threshold - 1);
            let crossed = resistance.push(Edge::Bottom, 1);

            // Assert
            assert!(!early);
            assert!(crossed);
        }
    }
}
