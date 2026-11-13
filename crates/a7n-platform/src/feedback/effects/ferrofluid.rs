// SPDX-License-Identifier: GPL-3.0-or-later

use std::f32::consts::FRAC_PI_2;

use tiny_skia::{
    BlendMode, GradientStop, LinearGradient, PathBuilder, Point, SpreadMode, Transform,
};

use super::{Canvas, Push};

const STEPS: u16 = 140;
pub(super) fn render(canvas: &mut Canvas<'_>, push: &Push) {
    if push.proximity < 0.01 {
        return;
    }
    let (start, end, half) = push.span(1.25);
    let mut ridge = PathBuilder::new();
    let mut body = PathBuilder::new();
    body.move_to(start, 0.0);
    for i in 0..=STEPS {
        let pos = start + (end - start) * f32::from(i) / f32::from(STEPS);
        let pull = (((pos - push.pointer).abs() / half * FRAC_PI_2)
            .cos()
            .max(0.0))
        .powf(2.2);
        let arg =
            (pos - push.pointer) * 0.085 * push.look.frequency + (push.phase * 1.4).sin() * 0.9;
        let spike =
            arg.cos().abs().powf(3.2) + (arg * 1.7 - push.phase * 1.1).sin().abs().powi(4) * 0.45;
        let height = push.look.depth * 1.35 * push.proximity * pull * (0.22 + 0.78 * spike);
        body.line_to(pos, height);
        if i == 0 {
            ridge.move_to(pos, height);
        } else {
            ridge.line_to(pos, height);
        }
    }
    body.line_to(end, 0.0);
    body.close();
    let tint = |t, alpha, factor| {
        let color = canvas.color(t, alpha);
        tiny_skia::Color::from_rgba(
            color.red() * factor,
            color.green() * factor,
            color.blue() * factor,
            color.alpha(),
        )
        .unwrap_or(color)
    };
    let stops = vec![
        GradientStop::new(0.0, tint(0.85, 0.95 * push.proximity, 0.25)),
        GradientStop::new(0.65, tint(0.45, 0.9 * push.proximity, 0.45)),
        GradientStop::new(1.0, tint(0.15, 0.95 * push.proximity, 1.0)),
    ];
    if let Some(shader) = LinearGradient::new(
        Point::from_xy(push.pointer, 0.0),
        Point::from_xy(push.pointer, push.look.depth * 1.35),
        stops,
        SpreadMode::Pad,
        Transform::identity(),
    ) {
        canvas.paint.shader = shader;
        canvas.paint.blend_mode = BlendMode::SourceOver;
        if let Some(body) = body.finish() {
            canvas.fill(&body);
        }
        canvas.paint.blend_mode = BlendMode::Plus;
    }
    if let Some(ridge) = ridge.finish() {
        canvas.solid(0.15, (0.95 * push.gain()).min(1.0));
        canvas.stroke(&ridge, 2.2);
        canvas.solid(0.45, (0.4 * push.gain()).min(1.0));
        canvas.stroke(&ridge, 7.0);
    }
}
