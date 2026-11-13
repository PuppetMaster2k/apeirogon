// SPDX-License-Identifier: GPL-3.0-or-later

//! Click-through native windows and the uncomposited landing shape.

use x11rb::connection::Connection;
use x11rb::protocol::shape::{ConnectionExt as _, SK, SO};
use x11rb::protocol::xproto::{
    AtomEnum, ClipOrdering, ConnectionExt as _, CreateWindowAux, PropMode, Rectangle, Window,
    WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use super::{Argb, Atoms, rgb_pixel};
use crate::feedback::Color;

/// Rasterize a circle into scanlines for SHAPE, without requiring a compositor.
pub(super) fn landing_shape(diameter: u16, dot: bool, scale: f64) -> Vec<Rectangle> {
    const RING_WIDTH: f64 = 2.0;
    let radius = f64::from(diameter) / 2.0;
    let inner = if dot {
        0.0
    } else {
        (radius - RING_WIDTH * scale).max(0.0)
    };
    let mut shape = Vec::new();
    for y in 0..diameter {
        let dy = f64::from(y) + 0.5 - radius;
        let mut start = None;
        for x in 0..=diameter {
            let dx = f64::from(x) + 0.5 - radius;
            let distance = dx * dx + dy * dy;
            let filled = x < diameter && distance <= radius * radius && distance >= inner * inner;
            if filled {
                start.get_or_insert(x);
            } else if let Some(from) = start.take() {
                shape.push(Rectangle {
                    x: i16::try_from(from).unwrap_or(0),
                    y: i16::try_from(y).unwrap_or(0),
                    width: x - from,
                    height: 1,
                });
            }
        }
    }
    shape
}

/// An unmapped, click-through, override-redirect window in the accent colour.
pub(super) fn create_overlay_window(
    connection: &RustConnection,
    screen_number: usize,
    atoms: &Atoms,
    argb: Option<&Argb>,
) -> Option<Window> {
    let screen = connection.setup().roots.get(screen_number)?;
    let window = connection.generate_id().ok()?;
    let mut aux = CreateWindowAux::new()
        .override_redirect(1)
        .background_pixel(rgb_pixel(Color::Accent.sample(0.0, true)));
    if let Some(argb) = argb {
        aux = aux
            .background_pixel(0)
            .border_pixel(0)
            .colormap(argb.colormap);
    }
    connection
        .create_window(
            if argb.is_some() {
                32
            } else {
                screen.root_depth
            },
            window,
            screen.root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            argb.map_or(screen.root_visual, |argb| argb.visual),
            &aux,
        )
        .ok()?
        .check()
        .ok()?;
    // An empty input region lets every click fall through to the windows below.
    connection
        .shape_rectangles(
            SO::SET,
            SK::INPUT,
            ClipOrdering::UNSORTED,
            window,
            0,
            0,
            &[],
        )
        .ok()?;
    connection
        .change_property32(
            PropMode::REPLACE,
            window,
            atoms.window_type,
            AtomEnum::ATOM,
            &[atoms.type_notification],
        )
        .ok()?;
    connection
        .change_property32(
            PropMode::REPLACE,
            window,
            atoms.state,
            AtomEnum::ATOM,
            &[atoms.state_above, atoms.skip_taskbar, atoms.skip_pager],
        )
        .ok()?;
    Some(window)
}
