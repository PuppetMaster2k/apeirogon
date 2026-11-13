// SPDX-License-Identifier: GPL-3.0-or-later

use std::f32::consts::FRAC_PI_2;

use tiny_skia::{GradientStop, LinearGradient, PathBuilder, Point, SpreadMode, Transform};

use super::{Canvas, Push};

pub(super) fn render(canvas: &mut Canvas<'_>, push: &Push) {
    if push.proximity < 0.01 {
        return;
    }
    let count = (34.0 * push.look.frequency).round().max(18.0) as u16;
    let (start, end, half) = push.span(1.25);
    let step = (end - start) / f32::from(count);
    for i in 0..=count {
        let i = f32::from(i);
        let base = start + i * step;
        let ratio = i / f32::from(count);
        let envelope = (((base - push.pointer).abs() / half * FRAC_PI_2)
            .cos()
            .max(0.0))
        .powf(1.4);
        if envelope < 0.02 {
            continue;
        }
        let sway = ((base + push.start) * 0.03 * push.look.frequency + push.phase * 1.3 + i * 0.4)
            .sin()
            * 14.0
            * envelope;
        let pos = (base + sway).clamp(-push.start, push.length - push.start);
        let height = 0.55
            + 0.30 * ((pos + push.start) * 0.04 * push.look.frequency - push.phase * 1.7).sin()
            + 0.15 * ((pos + push.start) * 0.11 * push.look.frequency + push.phase * 2.3).cos();
        let ray_height =
            push.look.depth * 1.55 * height * envelope * (0.45 + 0.55 * push.proximity);
        let a = Point::from_xy(pos, 0.0);
        let b = Point::from_xy(pos + sway * 0.5, ray_height);
        let t = (ratio * 0.75 + 0.15 * (push.phase * 0.8 + i * 0.25).sin() + 1.0) % 1.0;
        let alpha = ((0.35 + 0.35 * height) * envelope * push.gain()).min(1.0);
        let stops = vec![
            GradientStop::new(0.0, canvas.color(t, alpha * 0.95)),
            GradientStop::new(0.45, canvas.color((t + 0.35) % 1.0, alpha * 0.55)),
            GradientStop::new(1.0, canvas.color((t + 0.35) % 1.0, 0.0)),
        ];
        if let Some(shader) =
            LinearGradient::new(a, b, stops, SpreadMode::Pad, Transform::identity())
        {
            canvas.paint.shader = shader;
            let mut path = PathBuilder::new();
            path.move_to(a.x, a.y);
            path.line_to(b.x, b.y);
            if let Some(path) = path.finish() {
                canvas.stroke(&path, (step * 1.55).max(3.0));
            }
        }
    }
}
