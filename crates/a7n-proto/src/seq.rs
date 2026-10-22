// SPDX-License-Identifier: MIT OR Apache-2.0

//! Wrapping comparisons for `motion_seq` and the cumulative motion counters
//! (PROTOCOL.md section 9).

const HALF_RANGE: u32 = 1 << 31;

/// True when `new` is ahead of `last` in serial-number order (RFC 1982): the
/// forward distance is non-zero and below half the range.
#[must_use]
pub fn is_newer(new: u32, last: u32) -> bool {
    let ahead = new.wrapping_sub(last);
    ahead != 0 && ahead < HALF_RANGE
}

/// Movement from counter `last` to counter `new`, with 32-bit wrapping.
#[must_use]
pub fn delta(new: i32, last: i32) -> i32 {
    new.wrapping_sub(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_newer_across_wrap_and_stale_follows_serial_order() {
        // Arrange
        let cases = [
            (1, 0, true),
            (0, u32::MAX, true),
            (5, 5, false),
            (u32::MAX, 0, false),
            (HALF_RANGE, 0, false),
        ];

        for (new, last, expected) in cases {
            // Act
            let result = is_newer(new, last);

            // Assert
            assert_eq!(result, expected, "is_newer({new}, {last})");
        }
    }

    #[test]
    fn delta_counter_wrapped_returns_small_step() {
        // Arrange
        let (last, new) = (i32::MAX - 2, i32::MIN + 2);

        // Act
        let result = delta(new, last);

        // Assert
        assert_eq!(result, 5);
    }
}
