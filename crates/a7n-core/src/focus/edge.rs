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

/// Push progress when the resistance is fully built up, in per-mille.
pub const PROGRESS_FULL: u16 = 1000;

/// Haptic ticks fire at each quarter of the push.
const PROGRESS_QUARTER: u16 = PROGRESS_FULL / 4;

/// Whether a quarter of the push was passed going from one progress to the next.
#[must_use]
pub fn quarter_crossed(from: u16, to: u16) -> bool {
    to / PROGRESS_QUARTER > from / PROGRESS_QUARTER
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

    /// The edge being pushed and how much resistance has built up, in
    /// per-mille of [`PROGRESS_FULL`]. `None` when nothing is pushing.
    #[must_use]
    pub fn progress(&self) -> Option<(Edge, u16)> {
        if self.edge == Edge::Unspecified || self.overshoot_px == 0 {
            return None;
        }
        let per_mille =
            u64::from(self.overshoot_px) * u64::from(PROGRESS_FULL) / u64::from(self.resistance_px);
        let per_mille = per_mille.min(u64::from(PROGRESS_FULL));
        Some((self.edge, u16::try_from(per_mille).unwrap_or(PROGRESS_FULL)))
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
    fn progress_push_builds_up_and_resets_with_the_push() {
        // Arrange
        let mut resistance = EdgeResistance::new(100);
        let mut seen = vec![resistance.progress()];

        // Act
        for (edge, px) in [(Edge::Right, 25), (Edge::Right, 50), (Edge::Left, 10)] {
            resistance.push(edge, px);
            seen.push(resistance.progress());
        }
        resistance.push(Edge::Left, 90);
        seen.push(resistance.progress());
        resistance.push(Edge::Left, 0);
        seen.push(resistance.progress());

        // Assert
        assert_eq!(
            seen,
            [
                None,
                Some((Edge::Right, 250)),
                Some((Edge::Right, 750)),
                Some((Edge::Left, 100)),
                None,
                None,
            ]
        );
    }

    #[test]
    fn quarter_crossed_progress_steps_ticks_once_per_quarter() {
        // Arrange
        let cases = [
            (0, 249, false),
            (200, 250, true),
            (250, 499, false),
            (240, 760, true),
            (750, 999, false),
            (999, 1000, true),
        ];

        for (from, to, expected) in cases {
            // Act
            let crossed = quarter_crossed(from, to);

            // Assert
            assert_eq!(crossed, expected, "{from} -> {to}");
        }
    }
}
