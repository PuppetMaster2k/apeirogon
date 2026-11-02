// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::{MachineId, Placement};
use eframe::egui;

use super::SNAP_DISTANCE_PX;
use super::snap::{Snapped, overlaps, snap};

pub(super) struct Drag {
    start: Placement,
    delta: [f64; 2],
    last_valid: Placement,
}

impl Drag {
    pub(super) fn new(start: Placement) -> Self {
        Self {
            start,
            delta: [0.0; 2],
            last_valid: start,
        }
    }

    pub(super) fn machine(&self) -> MachineId {
        self.start.id
    }

    pub(super) fn update(
        &mut self,
        placement: &mut Placement,
        others: &[Placement],
        delta: egui::Vec2,
        scale: f64,
    ) -> Snapped {
        let raw = self.advance(delta, scale);
        let (candidate, snapped) = snap(raw, others, f64::from(SNAP_DISTANCE_PX) / scale);
        *placement = candidate;
        if !overlaps(&candidate, others) {
            self.last_valid = candidate;
        }
        snapped
    }

    pub(super) fn finish(self) -> Placement {
        self.last_valid
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "float-to-int casts saturate at the layout coordinate limits"
    )]
    fn advance(&mut self, delta: egui::Vec2, scale: f64) -> Placement {
        self.delta[0] += f64::from(delta.x) / scale;
        self.delta[1] += f64::from(delta.y) / scale;
        Placement {
            x: (f64::from(self.start.x) + self.delta[0]).round() as i32,
            y: (f64::from(self.start.y) + self.delta[1]).round() as i32,
            ..self.start
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement(id: u8, x: i32) -> Placement {
        Placement {
            id: MachineId::try_from([id; 16].as_slice()).unwrap(),
            x,
            y: 0,
            width: 100,
            height: 100,
        }
    }

    #[test]
    fn update_overlapping_drag_shows_overlapping_position() {
        // Arrange
        let others = [placement(1, 0)];
        let mut current = placement(2, 150);
        let mut drag = Drag::new(current);

        // Act
        drag.update(&mut current, &others, egui::vec2(-100.0, 0.0), 1.0);

        // Assert
        assert_eq!(current.x, 50);
        assert!(overlaps(&current, &others));
    }

    #[test]
    fn finish_overlapping_drag_returns_last_valid_or_start_position() {
        // Arrange
        let others = [placement(1, 0)];
        for (valid_delta, expected) in [(0.0, 150), (50.0, 200)] {
            let mut current = placement(2, 150);
            let mut drag = Drag::new(current);
            drag.update(&mut current, &others, egui::vec2(valid_delta, 0.0), 1.0);
            drag.update(
                &mut current,
                &others,
                egui::vec2(-100.0 - valid_delta, 0.0),
                1.0,
            );

            // Act
            let final_placement = drag.finish();

            // Assert
            assert_eq!(final_placement.x, expected);
        }
    }

    #[test]
    fn finish_valid_drag_keeps_final_snapped_position() {
        // Arrange
        let others = [placement(1, 0)];
        let mut current = placement(2, 150);
        let mut drag = Drag::new(current);
        drag.update(&mut current, &others, egui::vec2(-42.0, 0.0), 1.0);

        // Act
        let final_placement = drag.finish();

        // Assert
        assert_eq!(final_placement.x, 100);
    }

    #[test]
    fn update_snapped_drag_accumulates_motion_from_start() {
        // Arrange
        let others = [placement(1, 0)];
        let mut current = placement(2, 100);
        let mut drag = Drag::new(current);

        // Act
        for _ in 0..65 {
            drag.update(&mut current, &others, egui::vec2(0.4, 0.0), 2.0);
        }

        // Assert
        assert_eq!(current.x, 113);
    }
}
