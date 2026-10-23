// SPDX-License-Identifier: GPL-3.0-or-later

//! Sub-pixel remainders carried between events so slow motion is not lost.

/// Removes and returns the whole part of `value`, leaving the remainder.
#[allow(clippy::cast_possible_truncation, reason = "clamped to the i32 range")]
fn take_whole(value: &mut f64) -> i32 {
    let whole = value
        .trunc()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX));
    *value -= whole;
    whole as i32
}

#[derive(Debug, Default)]
pub(super) struct MotionAccumulator {
    x: f64,
    y: f64,
}

impl MotionAccumulator {
    /// Adds a raw (unaccelerated) delta and returns the whole pixels now due.
    pub(super) fn push(&mut self, dx: f64, dy: f64) -> (i32, i32) {
        self.x += dx;
        self.y += dy;
        (take_whole(&mut self.x), take_whole(&mut self.y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_fractional_motion_adds_up_to_one_pixel() {
        // Arrange
        let mut motion = MotionAccumulator::default();

        // Act
        let steps = [(0.4, 0.0), (0.4, 0.0), (0.4, 0.0)].map(|(dx, dy)| motion.push(dx, dy));

        // Assert
        assert_eq!(steps, [(0, 0), (0, 0), (1, 0)]);
    }

    #[test]
    fn push_negative_motion_keeps_negative_remainder() {
        // Arrange
        let mut motion = MotionAccumulator::default();

        // Act
        let steps = [(-0.6, 0.0), (-0.6, 0.0), (-0.9, 0.0)].map(|(dx, dy)| motion.push(dx, dy));

        // Assert
        assert_eq!(steps, [(0, 0), (-1, 0), (-1, 0)]);
    }
}
