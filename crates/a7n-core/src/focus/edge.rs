// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::{Duration, Instant};

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

/// Pressure lost each second, in pixels.
pub const PRESSURE_DECAY_PX_PER_SECOND: u32 = 240;

// At the default resistance, each tick fades the glow by five percent.
const PRESSURE_TICK: Duration = Duration::from_millis(25);
// Nanosecond units retain subpixel decay without rounding at each input event.
const PRESSURE_UNITS_PER_PX: u128 = Duration::from_secs(1).as_nanos();

/// Pressure on one edge, reduced by elapsed time and inward motion.
#[derive(Debug)]
pub struct EdgeResistance {
    resistance_px: u32,
    edge: Edge,
    pressure: u128,
    updated_at: Option<Instant>,
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
            pressure: 0,
            updated_at: None,
        }
    }

    /// Applies signed outward travel after time decay; changing edges resets pressure.
    pub fn push(&mut self, edge: Edge, travel_px: i32, now: Instant) -> bool {
        self.poll(now);
        if edge != self.edge || edge == Edge::Unspecified {
            self.edge = edge;
            self.pressure = 0;
        }
        if edge == Edge::Unspecified {
            return false;
        }
        let travel = u128::from(travel_px.unsigned_abs()) * PRESSURE_UNITS_PER_PX;
        if travel_px < 0 {
            self.pressure = self.pressure.saturating_sub(travel);
        } else {
            self.pressure += travel;
        }
        if self.pressure < u128::from(self.resistance_px) * PRESSURE_UNITS_PER_PX {
            return false;
        }
        self.pressure = 0;
        true
    }

    /// Applies elapsed decay, preserving fractional pixels across frequent updates.
    pub fn poll(&mut self, now: Instant) {
        if let Some(previous) = self.updated_at {
            let decay = now
                .saturating_duration_since(previous)
                .as_nanos()
                .saturating_mul(u128::from(PRESSURE_DECAY_PX_PER_SECOND));
            self.pressure = self.pressure.saturating_sub(decay);
        }
        self.updated_at = Some(now);
    }

    /// The next feedback refresh, only while pressure remains.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        self.updated_at
            .filter(|_| self.pressure > 0)
            .map(|now| now + PRESSURE_TICK)
    }

    /// The edge being pushed and how much resistance has built up, in
    /// per-mille of [`PROGRESS_FULL`]. Call `poll` or `push` with the current time first.
    /// `None` when no pressure remains.
    #[must_use]
    pub fn progress(&self) -> Option<(Edge, u16)> {
        if self.edge == Edge::Unspecified || self.pressure == 0 {
            return None;
        }
        let per_mille = self.pressure * u128::from(PROGRESS_FULL)
            / (u128::from(self.resistance_px) * PRESSURE_UNITS_PER_PX);
        let per_mille = per_mille.min(u128::from(PROGRESS_FULL));
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
        let now = Instant::now();
        let mut resistance = EdgeResistance::new(10);
        let mut buttons = HeldButtons::default();
        let button = |down| ButtonEvent {
            button: Button::Left.into(),
            down,
        };
        resistance.push(Edge::Right, 9, now);
        buttons.apply(&button(true));

        // Act
        let crossed = [(20, None), (9, Some(false)), (1, None)].map(|(px, down)| {
            if let Some(down) = down {
                buttons.apply(&button(down));
            }
            if buttons.any_held() {
                return resistance.push(Edge::Unspecified, 0, now);
            }
            resistance.push(Edge::Right, px, now)
        });

        // Assert
        assert_eq!(crossed, [false, false, true]);
    }

    #[test]
    fn push_accumulated_overshoot_crosses_at_threshold_and_resets() {
        // Arrange
        let now = Instant::now();
        let mut resistance = EdgeResistance::new(10);

        // Act
        let crossed = [3, 6, 1, 1].map(|px| resistance.push(Edge::Right, px, now));

        // Assert
        assert_eq!(crossed, [false, false, true, false]);
    }

    #[test]
    fn push_edge_change_discards_previous_pressure() {
        // Arrange
        let reset_cases = [(Edge::Unspecified, 5), (Edge::Left, 1)];

        for (edge, overshoot) in reset_cases {
            let now = Instant::now();
            let mut resistance = EdgeResistance::new(10);
            resistance.push(Edge::Right, 9, now);

            // Act
            let reset_crossed = resistance.push(edge, overshoot, now);
            let crossed = resistance.push(Edge::Right, 1, now);

            // Assert
            assert!(!reset_crossed);
            assert!(!crossed);
        }
    }

    #[test]
    fn progress_push_builds_up_and_resets_with_the_push() {
        // Arrange
        let now = Instant::now();
        let mut resistance = EdgeResistance::new(100);
        let mut seen = vec![resistance.progress()];

        // Act
        for (edge, px) in [(Edge::Right, 25), (Edge::Right, 50), (Edge::Left, 10)] {
            resistance.push(edge, px, now);
            seen.push(resistance.progress());
        }
        resistance.push(Edge::Left, 90, now);
        seen.push(resistance.progress());
        resistance.push(Edge::Left, 0, now);
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
    fn push_jitter_preserves_pressure_until_threshold() {
        // Arrange
        let now = Instant::now();
        let mut resistance = EdgeResistance::new(30);

        // Act
        let crossed = [10, 0, -1, 10, 0, -1, 10, 2].map(|px| resistance.push(Edge::Right, px, now));

        // Assert
        assert_eq!(
            crossed,
            [false, false, false, false, false, false, false, true]
        );
        assert_eq!(resistance.progress(), None);
        assert_eq!(resistance.next_deadline(), None);
    }

    #[test]
    fn push_inward_travel_subtracts_pressure_and_floors_at_zero() {
        // Arrange
        let now = Instant::now();
        for (travel, expected) in [
            (0, Some((Edge::Right, 600))),
            (-10, Some((Edge::Right, 500))),
            (-80, None),
            (i32::MIN, None),
        ] {
            let mut resistance = EdgeResistance::new(100);
            resistance.push(Edge::Right, 60, now);

            // Act
            let crossed = resistance.push(Edge::Right, travel, now);

            // Assert
            assert!(!crossed);
            assert_eq!(resistance.progress(), expected);
        }
    }

    #[test]
    fn poll_elapsed_time_decays_pressure_independent_of_update_frequency() {
        // Arrange
        let now = Instant::now();
        for steps in [1, 250] {
            let mut resistance = EdgeResistance::new(120);
            resistance.push(Edge::Right, 119, now);

            // Act
            for step in 1..=steps {
                resistance.poll(now + Duration::from_millis(250 * step / steps));
            }
            let halfway = resistance.progress();
            resistance.poll(now + Duration::from_millis(500));

            // Assert
            assert_eq!(halfway, Some((Edge::Right, 491)));
            assert_eq!(resistance.progress(), None);
            assert_eq!(resistance.next_deadline(), None);
        }
    }

    #[test]
    fn push_after_pause_applies_decay_before_outward_travel() {
        // Arrange
        let now = Instant::now();
        let mut resistance = EdgeResistance::new(120);
        resistance.push(Edge::Right, 100, now);

        // Act
        let crossed = resistance.push(Edge::Right, 20, now + Duration::from_millis(250));

        // Assert
        assert!(!crossed);
        assert_eq!(resistance.progress(), Some((Edge::Right, 500)));
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
