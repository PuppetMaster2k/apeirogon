// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_proto::limits::SCREEN_MM_MAX;

/// Invalid physical desktop sizes are unknown rather than protocol errors.
#[must_use]
pub fn screen_size_mm(size: (u32, u32)) -> Option<(u32, u32)> {
    let (width, height) = size;
    (width > 0 && height > 0 && width <= SCREEN_MM_MAX && height <= SCREEN_MM_MAX).then_some(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_size_mm_received_sizes_keeps_only_valid_dimensions() {
        // Arrange
        let cases = [
            ((310, 174), Some((310, 174))),
            (
                (SCREEN_MM_MAX, SCREEN_MM_MAX),
                Some((SCREEN_MM_MAX, SCREEN_MM_MAX)),
            ),
            ((0, 174), None),
            ((310, 0), None),
            ((SCREEN_MM_MAX + 1, 174), None),
            ((310, SCREEN_MM_MAX + 1), None),
        ];
        for ((width, height), expected) in cases {
            let wire = a7n_proto::v1::ScreenSize { width, height };

            // Act
            let actual = screen_size_mm((wire.width, wire.height));

            // Assert
            assert_eq!(actual, expected);
        }
    }
}
