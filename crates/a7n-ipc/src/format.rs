// SPDX-License-Identifier: GPL-3.0-or-later

const KIBIBYTE: u64 = 1024;
const SIZE_UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
const TENTHS: u128 = 10;

/// Binary units with one decimal, using integer math so nothing is rounded up.
#[must_use]
pub fn format_size(bytes: u64) -> String {
    if bytes < KIBIBYTE {
        return format!("{bytes} B");
    }
    let mut tenths = u128::from(bytes) * TENTHS;
    let mut unit = "";
    for next in SIZE_UNITS {
        tenths /= u128::from(KIBIBYTE);
        unit = next;
        if tenths < u128::from(KIBIBYTE) * TENTHS {
            break;
        }
    }
    format!("{}.{} {unit}", tenths / TENTHS, tenths % TENTHS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_size_byte_counts_use_binary_units_without_rounding_up() {
        // Arrange
        let cases = [
            (0, "0 B"),
            (1023, "1023 B"),
            (1024, "1.0 KiB"),
            (1536, "1.5 KiB"),
            (1024 * 1024 - 1, "1023.9 KiB"),
            (5 * 1024 * 1024, "5.0 MiB"),
            (1 << 40, "1.0 TiB"),
        ];
        for (bytes, expected) in cases {
            // Act
            let result = format_size(bytes);

            // Assert
            assert_eq!(result, expected);
        }
    }
}
