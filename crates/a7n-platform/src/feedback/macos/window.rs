// SPDX-License-Identifier: GPL-3.0-or-later

//! Click-through panels and `AppKit` screen geometry.

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSImageView, NSPanel, NSScreen, NSStatusWindowLevel,
    NSWindowCollectionBehavior, NSWindowSharingType, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use super::super::macos_geometry::Rect;

/// A borderless panel that cannot become key, ignores the mouse, floats over
/// every Space and full-screen app, and is excluded from screen capture.
pub(super) fn make_panel(mtm: MainThreadMarker, content: &NSImageView) -> Retained<NSPanel> {
    let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        mtm.alloc::<NSPanel>(),
        NSRect::ZERO,
        style,
        NSBackingStoreType::Buffered,
        false,
    );
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    panel.setHasShadow(false);
    panel.setIgnoresMouseEvents(true);
    panel.setLevel(NSStatusWindowLevel);
    panel.setSharingType(NSWindowSharingType::None);
    panel.setHidesOnDeactivate(false);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    panel.setContentView(Some(content));
    panel
}

pub(super) fn image_view(mtm: MainThreadMarker) -> Retained<NSImageView> {
    NSImageView::initWithFrame(mtm.alloc::<NSImageView>(), NSRect::ZERO)
}

// Screen dimensions are finite, positive OS values in points.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(super) fn pixel_size(value: f64) -> u32 {
    value.round().max(1.0) as u32
}

/// All screen frames in `AppKit` space, the primary screen first.
pub(super) fn screen_frames(mtm: MainThreadMarker) -> Vec<Rect> {
    NSScreen::screens(mtm)
        .iter()
        .map(|screen| {
            let frame = screen.frame();
            Rect {
                x: frame.origin.x,
                y: frame.origin.y,
                width: frame.size.width,
                height: frame.size.height,
            }
        })
        .collect()
}

pub(super) fn ns_rect(rect: Rect) -> NSRect {
    NSRect::new(
        NSPoint::new(rect.x, rect.y),
        NSSize::new(rect.width, rect.height),
    )
}
