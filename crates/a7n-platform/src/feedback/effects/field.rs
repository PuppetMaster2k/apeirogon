// SPDX-License-Identifier: GPL-3.0-or-later

//! Reused 0.4-resolution pixel field, bilinearly enlarged like the HTML canvas.
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

use super::{Canvas, Push, Style, clouds, membrane};

const RATIO: f32 = 320.0 / 800.0;
#[derive(Default)]
pub(super) struct State {
    pixels: Option<Pixmap>,
}
pub(super) fn render(canvas: &mut Canvas<'_>, push: &Push, state: &mut State) {
    let scale = canvas
        .transform
        .sx
        .abs()
        .max(canvas.transform.ky.abs())
        .max(1.0);
    let width = (canvas.pixmap.width() as f32 / scale * RATIO)
        .ceil()
        .max(1.0) as u32;
    let height = (canvas.pixmap.height() as f32 / scale * RATIO)
        .ceil()
        .max(1.0) as u32;
    if state
        .pixels
        .as_ref()
        .is_none_or(|f| f.width() != width || f.height() != height)
    {
        state.pixels = Pixmap::new(width, height);
    }
    let Some(field) = &mut state.pixels else {
        return;
    };
    field.fill(tiny_skia::Color::TRANSPARENT);
    let mut pointer = [tiny_skia::Point::from_xy(push.pointer, 0.0)];
    canvas.transform.map_points(&mut pointer);
    let mx = pointer[0].x / scale * RATIO;
    let my = pointer[0].y / scale * RATIO;
    let radius = push.look.radius * RATIO;
    let depth = push.look.depth * RATIO;
    // The pixel reference keeps pointerActive at one; pressure controls cursorTrigger only.
    let global_gain = (0.25 + 0.75 * push.proximity) * push.look.intensity;
    for (index, px) in field.data_mut().chunks_exact_mut(4).enumerate() {
        let x = (index % width as usize) as f32;
        let y = (index / width as usize) as f32;
        let dx = x - mx;
        let dy = y - my;
        let dist = dx.hypot(dy);
        // Only the commanded edge contributes, including at monitor corners.
        let edge_dist = match push.edge {
            super::Edge::Top => y,
            super::Edge::Bottom => height as f32 - 1.0 - y,
            super::Edge::Right => width as f32 - 1.0 - x,
            _ => x,
        };
        if dist > radius || edge_dist >= depth {
            continue;
        }
        let edge_norm = 1.0 - edge_dist / depth;
        let falloff = edge_norm.powi(2)
            * (3.0 - 2.0 * edge_norm)
            * (1.0 - dist / radius).powi(2)
            * global_gain;
        let (t, alpha) = if push.look.style == Style::Membrane {
            membrane::sample(x, y, dx, dy, push.phase, push.look.frequency, edge_norm)
        } else {
            clouds::sample(x, y, push.phase, push.look.frequency)
        };
        let color = canvas.color(t, alpha * falloff).premultiply().to_color_u8();
        px.copy_from_slice(&[color.red(), color.green(), color.blue(), color.alpha()]);
    }
    canvas.pixmap.draw_pixmap(
        0,
        0,
        field.as_ref(),
        &PixmapPaint {
            quality: FilterQuality::Bilinear,
            ..PixmapPaint::default()
        },
        Transform::from_scale(scale / RATIO, scale / RATIO),
        None,
    );
}
