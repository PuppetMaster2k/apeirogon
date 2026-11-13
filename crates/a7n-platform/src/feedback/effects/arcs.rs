// SPDX-License-Identifier: GPL-3.0-or-later

use tiny_skia::{PathBuilder, Point, Shader};

use super::{Canvas, Push, Random};

const MAX_POINTS: usize = 33;
fn bolt(
    canvas: &mut Canvas<'_>,
    ends: [Point; 2],
    displace: f32,
    depth: usize,
    color: f32,
    alpha: f32,
    random: &mut Random,
) {
    let mut points = [Point::from_xy(0.0, 0.0); MAX_POINTS];
    points[0] = ends[0];
    points[1] = ends[1];
    let mut count = 2;
    let mut displacement = displace;
    for _ in 0..depth {
        for i in (0..count - 1).rev() {
            let a = points[i];
            let b = points[i + 1];
            let dx = b.x - a.x;
            let dy = b.y - a.y;
            let length = dx.hypot(dy).max(f32::EPSILON);
            let offset = (random.unit() - 0.5) * displacement;
            points[i * 2 + 2] = b;
            points[i * 2 + 1] = Point::from_xy(
                a.x.midpoint(b.x) - dy / length * offset,
                a.y.midpoint(b.y) + dx / length * offset,
            );
            points[i * 2] = a;
        }
        count = count * 2 - 1;
        displacement *= 0.52;
    }
    let mut path = PathBuilder::new();
    path.move_to(points[0].x, points[0].y);
    for point in &points[1..count] {
        path.line_to(point.x, point.y);
    }
    if let Some(path) = path.finish() {
        canvas.solid(color, alpha * 0.35);
        canvas.stroke(&path, 6.5);
        canvas.solid(color, alpha * 0.85);
        canvas.stroke(&path, 2.4);
        canvas.paint.shader = Shader::SolidColor(tiny_skia::Color::from_rgba8(
            255,
            255,
            255,
            (alpha * 0.95 * 255.0).clamp(0.0, 255.0).round() as u8,
        ));
        canvas.stroke(&path, 0.9);
    }
}
pub(super) fn render(canvas: &mut Canvas<'_>, push: &Push, random: &mut Random) {
    if push.proximity < 0.02 {
        return;
    }
    canvas.spot(push, 0.45, &[(0.0, 0.25, 0.55), (1.0, 0.0, 0.0)]);
    if !push.animate {
        return;
    }
    let count = (4.0 * push.look.frequency).round().max(2.0) as u16;
    let half = push.look.radius * 0.95;
    for i in 0..count {
        let pulse = (push.phase * 14.0 + f32::from(i) * 2.7).sin();
        if pulse < -0.25 {
            continue;
        }
        let color = (f32::from(i) / f32::from(count) + push.phase * 0.2) % 1.0;
        let alpha = ((0.55 + 0.45 * pulse) * push.gain()).min(1.0);
        let clamp = |pos: f32| {
            pos.clamp(
                2.0 - push.start,
                (push.length - push.start - 2.0).max(2.0 - push.start),
            )
        };
        let (a, b, displacement, depth) = if i % 2 == 0 {
            let offset = (random.unit() - 0.5) * half * 1.4;
            let direction = if random.unit() > 0.5 { 1.0 } else { -1.0 };
            let other = offset + direction * (35.0 + random.unit() * half * 0.7);
            (
                Point::from_xy(clamp(push.pointer + offset), 2.0),
                Point::from_xy(clamp(push.pointer + other), 2.0),
                push.look.depth * 0.55,
                5,
            )
        } else {
            let offset = (random.unit() - 0.5) * half * 0.9;
            (
                Point::from_xy(clamp(push.pointer + offset), 1.0),
                Point::from_xy(
                    push.pointer + (random.unit() - 0.5) * 14.0,
                    (random.unit() - 0.5) * 14.0,
                ),
                push.look.depth * 0.42,
                4,
            )
        };
        bolt(canvas, [a, b], displacement, depth, color, alpha, random);
    }
}
