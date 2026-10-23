// SPDX-License-Identifier: GPL-3.0-or-later

//! Sub-unit remainders carried between events so smooth scrolling is not lost.

use a7n_proto::v1::ScrollUnit;

/// Pixels of smooth scrolling that make one wheel notch.
pub(super) const SCROLL_PIXELS_PER_LINE: f64 = 15.0;

/// Bounds the clicks one hostile or buggy event can inject.
pub(super) const MAX_LINES_PER_EVENT: f64 = 100.0;

/// Removes and returns the whole part of `value`, leaving the remainder.
#[allow(clippy::cast_possible_truncation, reason = "clamped to the i32 range")]
fn take_whole(value: &mut f64) -> i32 {
    let whole = value
        .trunc()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX));
    *value -= whole;
    whole as i32
}

/// Remainders are kept in pixels, where whole notches are exact multiples of
/// [`SCROLL_PIXELS_PER_LINE`], so repeated additions do not drift.
#[derive(Debug, Default)]
pub(super) struct ScrollAccumulator {
    x: f64,
    y: f64,
}

impl ScrollAccumulator {
    /// Adds a scroll delta and returns the whole wheel notches now due.
    /// Non-finite deltas and an unspecified unit add nothing.
    pub(super) fn push(&mut self, dx: f32, dy: f32, unit: ScrollUnit) -> (i32, i32) {
        let pixels_per_unit = match unit {
            ScrollUnit::Line => SCROLL_PIXELS_PER_LINE,
            ScrollUnit::Pixel => 1.0,
            ScrollUnit::Unspecified => return (0, 0),
        };
        self.x += pixels(dx, pixels_per_unit);
        self.y += pixels(dy, pixels_per_unit);
        (take_notches(&mut self.x), take_notches(&mut self.y))
    }
}

fn pixels(delta: f32, pixels_per_unit: f64) -> f64 {
    if !delta.is_finite() {
        return 0.0;
    }
    let limit = MAX_LINES_PER_EVENT * SCROLL_PIXELS_PER_LINE;
    (f64::from(delta) * pixels_per_unit).clamp(-limit, limit)
}

fn take_notches(pixels: &mut f64) -> i32 {
    let mut notches = (*pixels / SCROLL_PIXELS_PER_LINE).trunc();
    *pixels -= notches * SCROLL_PIXELS_PER_LINE;
    take_whole(&mut notches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_pixel_scroll_below_one_notch_produces_nothing() {
        // Arrange
        let mut scroll = ScrollAccumulator::default();

        // Act
        let notches = scroll.push(0.0, 14.0, ScrollUnit::Pixel);

        // Assert
        assert_eq!(notches, (0, 0));
    }

    #[test]
    fn push_pixel_scroll_crossing_threshold_emits_one_notch_and_keeps_remainder() {
        // Arrange
        let mut scroll = ScrollAccumulator::default();
        scroll.push(0.0, 10.0, ScrollUnit::Pixel);

        // Act
        let crossed = scroll.push(0.0, 10.0, ScrollUnit::Pixel);
        let remainder = scroll.push(0.0, 10.0, ScrollUnit::Pixel);

        // Assert
        assert_eq!(crossed, (0, 1));
        assert_eq!(remainder, (0, 1));
    }

    #[test]
    fn push_line_scroll_hostile_values_are_bounded() {
        // Arrange
        let mut scroll = ScrollAccumulator::default();

        // Act
        let huge = scroll.push(f32::NAN, 1e30, ScrollUnit::Line);

        // Assert
        assert_eq!(huge, (0, 100));
    }
}
