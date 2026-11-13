// SPDX-License-Identifier: GPL-3.0-or-later

use std::f32::consts::PI;

use tiny_skia::{GradientStop, PathBuilder, Point};

use super::{Canvas, Push};

// Canvas arcs become cubic Beziers, split into at most quarter-circle segments.
fn arc(path: &mut PathBuilder, center: Point, radius: f32, start: f32, end: f32) {
    let steps = ((end - start).abs() / (PI / 2.0)).ceil().max(1.0) as u16;
    for i in 0..steps {
        let a = start + (end - start) * f32::from(i) / f32::from(steps);
        let b = start + (end - start) * f32::from(i + 1) / f32::from(steps);
        let k = 4.0 / 3.0 * ((b - a) / 4.0).tan();
        path.cubic_to(
            center.x + radius * (a.cos() - k * a.sin()),
            center.y + radius * (a.sin() + k * a.cos()),
            center.x + radius * (b.cos() + k * b.sin()),
            center.y + radius * (b.sin() - k * b.cos()),
            center.x + radius * b.cos(),
            center.y + radius * b.sin(),
        );
    }
}
pub(super) fn render(canvas: &mut Canvas<'_>, push: &Push) {
    if push.proximity < 0.01 {
        return;
    }
    let radius = (push.look.depth * 1.45 + push.look.radius * 0.45) * (0.4 + 0.6 * push.proximity);
    let center = Point::from_xy(push.pointer, 0.0);
    let warm = tiny_skia::Color::from_rgba8(
        255,
        250,
        235,
        ((0.85 * push.gain()).min(1.0) * 255.0).round() as u8,
    );
    let stops = vec![
        GradientStop::new(0.0, warm),
        GradientStop::new(0.2, canvas.color(0.05, 0.7 * push.gain())),
        GradientStop::new(0.55, canvas.color(0.5, 0.32 * push.gain())),
        GradientStop::new(1.0, canvas.color(0.95, 0.0)),
    ];
    canvas.radial(center, 0.0, radius * 1.35, stops);
    canvas.circle(center.x, center.y, radius * 1.35);
    let count = (14.0 * push.look.frequency).round().max(6.0) as u16;
    for i in 0..count {
        let ratio = f32::from(i) / f32::from(count);
        let i = f32::from(i);
        let angle =
            PI / 2.0 + (ratio - 0.5) * PI * 0.92 + (push.phase * 0.7 + i * 2.1).sin() * 0.14;
        let spread = PI / f32::from(count) * (0.35 + 0.25 * (push.phase * 1.3 + i).sin());
        let length = radius * (0.85 + 0.45 * (push.phase * 0.9 + i * 1.7).cos());
        let t = (ratio + (push.phase * 0.3 + i).sin() * 0.2 + 1.0) % 1.0;
        let alpha = ((0.16 + 0.14 * (push.phase * 1.6 + i * 3.0).sin()) * push.gain()).min(1.0);
        canvas.radial(
            center,
            4.0,
            length,
            vec![
                GradientStop::new(0.0, canvas.color(t, alpha * 1.5)),
                GradientStop::new(1.0, canvas.color(t, 0.0)),
            ],
        );
        let mut path = PathBuilder::new();
        path.move_to(center.x, center.y);
        path.line_to(
            center.x + length * (angle - spread).cos(),
            center.y + length * (angle - spread).sin(),
        );
        arc(&mut path, center, length, angle - spread, angle + spread);
        path.close();
        if let Some(path) = path.finish() {
            canvas.fill(&path);
        }
    }
}
