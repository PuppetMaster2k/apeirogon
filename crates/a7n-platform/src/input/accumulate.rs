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
    fn push_fractional_motion_keeps_signed_remainder() {
        // Arrange
        let mut positive = MotionAccumulator::default();
        let mut negative = MotionAccumulator::default();

        // Act
        let up = [0.4, 0.4, 0.4].map(|dx| positive.push(dx, 0.0));
        let down = [-0.6, -0.6, -0.9].map(|dx| negative.push(dx, 0.0));

        // Assert
        assert_eq!(up, [(0, 0), (0, 0), (1, 0)]);
        assert_eq!(down, [(0, 0), (-1, 0), (-1, 0)]);
    }
}
