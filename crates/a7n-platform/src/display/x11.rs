// SPDX-License-Identifier: GPL-3.0-or-later

use x11rb::connection::Connection;

pub(super) fn physical_size_mm() -> Option<(u32, u32)> {
    let (connection, screen) = x11rb::connect(None).ok()?;
    let screen = connection.setup().roots.get(screen)?;
    let pixels = (
        f64::from(screen.width_in_pixels),
        f64::from(screen.height_in_pixels),
    );
    super::scale_desktop_mm(
        (
            f64::from(screen.width_in_millimeters),
            f64::from(screen.height_in_millimeters),
        ),
        pixels,
        pixels,
    )
}
