// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_proto::{limits, v1::Edge};

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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn push_edge_change_discards_previous_overshoot() {
        // Arrange
        let mut resistance = EdgeResistance::new(10);
        resistance.push(Edge::Right, 9);

        // Act
        let crossed = [1, 9].map(|px| resistance.push(Edge::Left, px));

        // Assert
        assert_eq!(crossed, [false, true]);
    }

    #[test]
    fn push_moving_away_discards_previous_overshoot() {
        // Arrange
        let reset_cases = [(Edge::Right, 0), (Edge::Right, -1), (Edge::Unspecified, 5)];

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
