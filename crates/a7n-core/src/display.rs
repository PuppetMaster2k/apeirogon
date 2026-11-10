// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_proto::limits::SCREEN_MM_MAX;

/// Invalid physical desktop sizes are unknown rather than protocol errors.
#[must_use]
pub fn screen_size_mm(size: (u32, u32)) -> Option<(u32, u32)> {
    let (width, height) = size;
    (width > 0 && height > 0 && width <= SCREEN_MM_MAX && height <= SCREEN_MM_MAX).then_some(size)
}
