// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_proto::v1::{ScrollEvent, ScrollUnit};

use super::MotionScale;

/// Receiver-side scroll scaling with independent pixel and line remainders.
#[derive(Debug)]
pub struct ScrollScale {
    pixels: MotionScale,
    lines: MotionScale,
}

impl ScrollScale {
    /// Speed is validated at the configuration boundary; reversal applies to both axes.
    #[must_use]
    pub fn new(speed: f64, reverse: bool) -> Self {
        let speed = if reverse { -speed } else { speed };
        Self {
            pixels: MotionScale::new(speed),
            lines: MotionScale::new(speed),
        }
    }

    #[allow(clippy::cast_precision_loss)] // ScrollEvent uses f32; whole deltas saturate at i32 bounds.
    pub fn apply(&mut self, event: &mut ScrollEvent) {
        let scale = match event.unit() {
            ScrollUnit::Pixel => &mut self.pixels,
            ScrollUnit::Line => &mut self.lines,
            ScrollUnit::Unspecified => return,
        };
        event.dx =
            MotionScale::axis(f64::from(event.dx), scale.speed, &mut scale.remainder.0) as f32;
        event.dy =
            MotionScale::axis(f64::from(event.dy), scale.speed, &mut scale.remainder.1) as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_fractional_scroll_retains_remainder_per_unit_and_reverses_axes() {
        // Arrange
        let mut scale = ScrollScale::new(0.25, true);
        let mut output = Vec::new();

        // Act
        for _ in 0..4 {
            for unit in [ScrollUnit::Line, ScrollUnit::Pixel] {
                let mut event = ScrollEvent {
                    dx: 1.0,
                    dy: -1.0,
                    unit: unit.into(),
                };
                scale.apply(&mut event);
                output.push((event.dx, event.dy));
            }
        }

        // Assert
        assert_eq!(
            output,
            [
                (0.0, 0.0),
                (0.0, 0.0),
                (0.0, 0.0),
                (0.0, 0.0),
                (0.0, 0.0),
                (0.0, 0.0),
                (-1.0, 1.0),
                (-1.0, 1.0)
            ]
        );
    }

    #[test]
    fn apply_non_finite_scroll_preserves_pending_fraction() {
        // Arrange
        let mut scale = ScrollScale::new(0.5, false);
        let mut event = ScrollEvent {
            dx: 1.0,
            dy: 1.0,
            unit: ScrollUnit::Line.into(),
        };
        scale.apply(&mut event);

        // Act
        let mut invalid = ScrollEvent {
            dx: f32::NAN,
            dy: f32::INFINITY,
            unit: ScrollUnit::Line.into(),
        };
        scale.apply(&mut invalid);
        event.dx = 1.0;
        event.dy = 1.0;
        scale.apply(&mut event);

        // Assert
        assert_eq!((invalid.dx, invalid.dy), (0.0, 0.0));
        assert_eq!((event.dx, event.dy), (1.0, 1.0));
    }
}
