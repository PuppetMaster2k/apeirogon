// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::OnceLock;

use objc2::{MainThreadMarker, sel};
use objc2_app_kit::NSScreen;
use objc2_foundation::NSObjectProtocol;

use super::Bounds;

static NOTCH: OnceLock<Option<Bounds>> = OnceLock::new();

pub(crate) fn init(mtm: MainThreadMarker) {
    NOTCH.get_or_init(|| find(mtm));
}

pub(super) fn snapshot() -> Option<Bounds> {
    NOTCH.get().copied().flatten()
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "screen coordinates and positive notch dimensions are checked before conversion"
)]
fn find(mtm: MainThreadMarker) -> Option<Bounds> {
    let screens = NSScreen::screens(mtm);
    let primary_height = screens.firstObject()?.frame().size.height;
    for screen in &screens {
        if !screen.respondsToSelector(sel!(auxiliaryTopLeftArea))
            || !screen.respondsToSelector(sel!(auxiliaryTopRightArea))
        {
            continue;
        }
        let left = screen.auxiliaryTopLeftArea();
        let right = screen.auxiliaryTopRightArea();
        let frame = screen.frame();
        let x = left.origin.x + left.size.width;
        let y = primary_height - (frame.origin.y + frame.size.height);
        let width = right.origin.x - x;
        let height = left.size.height.min(right.size.height);
        if ![x, y, width, height].iter().all(|v| v.is_finite())
            || x < f64::from(i32::MIN)
            || x > f64::from(i32::MAX)
            || y < f64::from(i32::MIN)
            || y > f64::from(i32::MAX)
            || width <= 0.0
            || width > f64::from(u32::MAX)
            || height <= 0.0
            || height > f64::from(u32::MAX)
            || left.size.width <= 0.0
            || right.size.width <= 0.0
        {
            continue;
        }
        return Some(Bounds {
            x: x as i32,
            y: y as i32,
            width: width as u32,
            height: height as u32,
        });
    }
    None
}
