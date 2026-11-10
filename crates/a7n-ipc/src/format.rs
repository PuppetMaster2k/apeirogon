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
