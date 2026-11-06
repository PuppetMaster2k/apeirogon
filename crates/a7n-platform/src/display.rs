// SPDX-License-Identifier: GPL-3.0-or-later

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;
#[cfg(all(unix, not(target_os = "macos")))]
mod x11;

/// Physical size of the whole virtual desktop, if the display reports it.
#[must_use]
pub fn physical_size_mm() -> Option<(u32, u32)> {
    platform_size_mm()
}

#[cfg(target_os = "macos")]
use macos::physical_size_mm as platform_size_mm;
#[cfg(windows)]
use windows::physical_size_mm as platform_size_mm;

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_size_mm() -> Option<(u32, u32)> {
    // shortcut: portal input does not bind wl_output; add Wayland sizes when it does.
    if crate::input::detect_backend() != crate::input::Backend::X11 {
        return None;
    }
    x11::physical_size_mm()
}

#[cfg(not(any(windows, unix)))]
fn platform_size_mm() -> Option<(u32, u32)> {
    None
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "rounded physical sides are finite, positive and bounded before conversion"
)]
fn scale_desktop_mm(
    primary_mm: (f64, f64),
    primary_pixels: (f64, f64),
    desktop_pixels: (f64, f64),
) -> Option<(u32, u32)> {
    let max = f64::from(a7n_proto::limits::SCREEN_MM_MAX);
    let scale_side = |mm: f64, primary: f64, desktop: f64| {
        if ![mm, primary, desktop]
            .iter()
            .all(|v| v.is_finite() && *v > 0.0)
            || mm > max
        {
            return None;
        }
        let side = mm * desktop / primary;
        (side >= 1.0 && side <= max).then(|| side.round() as u32)
    };
    Some((
        scale_side(primary_mm.0, primary_pixels.0, desktop_pixels.0)?,
        scale_side(primary_mm.1, primary_pixels.1, desktop_pixels.1)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_desktop_mm_desktop_and_invalid_sizes_scale_or_return_unknown() {
        // Arrange
        let max = f64::from(a7n_proto::limits::SCREEN_MM_MAX);
        let cases = [
            (
                (310.0, 174.0),
                (1920.0, 1080.0),
                (3840.0, 1080.0),
                Some((620, 174)),
            ),
            ((0.0, 174.0), (1920.0, 1080.0), (1920.0, 1080.0), None),
            ((310.0, 174.0), (0.0, 1080.0), (1920.0, 1080.0), None),
            ((310.0, 174.0), (1920.0, 1080.0), (1920.0, 0.0), None),
            ((max + 1.0, 174.0), (1920.0, 1080.0), (1920.0, 1080.0), None),
            ((max, 174.0), (1920.0, 1080.0), (3840.0, 1080.0), None),
            ((f64::NAN, 174.0), (1920.0, 1080.0), (1920.0, 1080.0), None),
            (
                (310.0, 174.0),
                (1920.0, f64::INFINITY),
                (1920.0, 1080.0),
                None,
            ),
        ];
        for (mm, primary, desktop, expected) in cases {
            // Act
            let actual = scale_desktop_mm(mm, primary, desktop);

            // Assert
            assert_eq!(actual, expected);
        }
    }
}
