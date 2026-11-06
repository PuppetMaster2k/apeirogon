// SPDX-License-Identifier: GPL-3.0-or-later

use objc2_core_graphics::{CGDisplayBounds, CGDisplayScreenSize, CGMainDisplayID};

pub(super) fn physical_size_mm() -> Option<(u32, u32)> {
    let bounds = crate::input::macos::display_bounds().ok()?;
    let primary = CGMainDisplayID();
    let mm = CGDisplayScreenSize(primary);
    // Use the same desktop coordinate units as input, including Retina scaling.
    let pixels = CGDisplayBounds(primary).size;
    super::scale_desktop_mm(
        (mm.width, mm.height),
        (pixels.width, pixels.height),
        (f64::from(bounds.width), f64::from(bounds.height)),
    )
}
