// SPDX-License-Identifier: GPL-3.0-or-later

use objc2_core_foundation::{
    CFArray, CFDictionary, CFNumber, CFRetained, CFString, CFType, CGPoint, CGRect,
};
use objc2_core_graphics::{
    CGDisplayBounds, CGError, CGGetDisplaysWithPoint, CGRectMakeWithDictionaryRepresentation,
    CGWindowListCopyWindowInfo, CGWindowListOption, kCGWindowBounds, kCGWindowLayer,
    kCGWindowOwnerName,
};

pub(super) fn focused_fullscreen_at(x: i32, y: i32) -> Option<bool> {
    let bounds = display_bounds_at(x, y)?;
    let windows = CGWindowListCopyWindowInfo(CGWindowListOption::OptionOnScreenOnly, 0)?;
    // SAFETY: the Core Graphics array contains retained CF objects.
    let windows = unsafe { CFRetained::cast_unchecked::<CFArray<CFType>>(windows) };
    for window in windows.iter() {
        let Ok(window) = window.downcast::<CFDictionary>() else {
            continue;
        };
        // SAFETY: Core Graphics window dictionaries have CFString keys and CF object values.
        let window =
            unsafe { CFRetained::cast_unchecked::<CFDictionary<CFString, CFType>>(window) };
        // SAFETY: these immutable Core Graphics CFString constants live for the process lifetime.
        let (owner, layer, rect) = unsafe { (kCGWindowOwnerName, kCGWindowLayer, kCGWindowBounds) };
        let Some(layer) = window
            .get(layer)
            .and_then(|v| v.downcast::<CFNumber>().ok())
            .and_then(|v| v.as_i32())
        else {
            continue;
        };
        if layer != 0 {
            continue;
        }
        let Some(owner) = window
            .get(owner)
            .and_then(|v| v.downcast::<CFString>().ok())
        else {
            continue;
        };
        let Some(rect) = window
            .get(rect)
            .and_then(|v| v.downcast::<CFDictionary>().ok())
        else {
            continue;
        };
        let mut window_bounds = CGRect::default();
        // SAFETY: rect is a checked CFDictionary and window_bounds is a writable CGRect.
        if !unsafe { CGRectMakeWithDictionaryRepresentation(Some(&rect), &raw mut window_bounds) } {
            continue;
        }
        return Some(owner.to_string() != super::GUI_NAME && window_bounds == bounds);
    }
    Some(false)
}

fn display_bounds_at(x: i32, y: i32) -> Option<CGRect> {
    let mut display = 0;
    let mut count = 0;
    // SAFETY: display and count are writable outputs; capacity is one display.
    let error = unsafe {
        CGGetDisplaysWithPoint(
            CGPoint::new(f64::from(x), f64::from(y)),
            1,
            &raw mut display,
            &raw mut count,
        )
    };
    (error == CGError::Success && count != 0).then(|| CGDisplayBounds(display))
}
