// SPDX-License-Identifier: GPL-3.0-or-later

//! Edge FX Studio: HTML equations in logical pixels, native frames in physical pixels.

// The reference uses floating point canvas geometry and bounded loop/index conversions.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use std::time::Duration;

use a7n_proto::v1::Edge;
use tiny_skia::{
    BlendMode, FillRule, GradientStop, LineCap, LineJoin, Paint, Path, PathBuilder, PixmapMut,
    Point, RadialGradient, Shader, SpreadMode, Stroke, Transform,
};

use super::raster::{self, EdgeEffect, Frame};
use super::timeline::{BLOOM_DURATION, REDUCED_DURATION};
use super::{Look, Style};

mod arcs;
mod aurora;
mod clouds;
mod ferrofluid;
mod field;
mod membrane;
mod sparks;
mod sunrise;
mod waves;

#[derive(Default)]
pub(super) struct State {
    sparks: sparks::State,
    field: field::State,
    random: Random,
    last: Option<f32>,
}

struct Random(u32);
impl Default for Random {
    fn default() -> Self {
        Self(0x92d6_8ca2)
    }
}
impl Random {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 8) as f32 / 16_777_216.0
    }
}

struct Push {
    look: Look,
    phase: f32,
    proximity: f32,
    pointer: f32,
    length: f32,
    start: f32,
    edge: Edge,
    animate: bool,
}
impl Push {
    fn span(&self, factor: f32) -> (f32, f32, f32) {
        let half = self.look.radius * factor;
        (
            (self.pointer - half).max(-self.start),
            (self.pointer + half).min(self.length - self.start),
            half,
        )
    }
    fn gain(&self) -> f32 {
        self.proximity * self.look.intensity
    }
}

struct Canvas<'a> {
    pixmap: PixmapMut<'a>,
    paint: Paint<'static>,
    transform: Transform,
    look: Look,
    dark: bool,
}
impl Canvas<'_> {
    fn color(&self, t: f32, alpha: f32) -> tiny_skia::Color {
        let [r, g, b] = self
            .look
            .color
            .sample((t.clamp(0.0, 1.0) * 255.0).floor() / 255.0, self.dark);
        tiny_skia::Color::from_rgba8(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
    }
    fn solid(&mut self, t: f32, alpha: f32) {
        self.paint.shader = Shader::SolidColor(self.color(t, alpha));
    }
    fn fill(&mut self, path: &Path) {
        self.pixmap
            .fill_path(path, &self.paint, FillRule::Winding, self.transform, None);
    }
    fn stroke(&mut self, path: &Path, width: f32) {
        self.pixmap.stroke_path(
            path,
            &self.paint,
            &Stroke {
                width,
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..Stroke::default()
            },
            self.transform,
            None,
        );
    }
    fn circle(&mut self, x: f32, y: f32, radius: f32) {
        if let Some(p) = PathBuilder::from_circle(x, y, radius) {
            self.fill(&p);
        }
    }
    fn radial(&mut self, center: Point, inner: f32, radius: f32, stops: Vec<GradientStop>) {
        if let Some(shader) = RadialGradient::new(
            center,
            inner,
            center,
            radius,
            stops,
            SpreadMode::Pad,
            Transform::identity(),
        ) {
            self.paint.shader = shader;
        }
    }
    fn spot(&mut self, p: &Push, factor: f32, stops: &[(f32, f32, f32)]) {
        let radius = p.look.radius * factor;
        let stops = stops
            .iter()
            .map(|&(at, t, a)| GradientStop::new(at, self.color(t, a * p.gain())))
            .collect();
        self.radial(Point::from_xy(p.pointer, 0.0), 0.0, radius, stops);
        self.circle(p.pointer, 0.0, radius);
    }
}

pub(super) fn departure(frame: &mut Frame, effect: EdgeEffect, seconds: f32) {
    let scale = frame.scale.max(1.0) as f32;
    let p = Push {
        look: effect.look,
        phase: if frame.animate {
            seconds * effect.look.speed * 1.6
        } else {
            0.0
        },
        proximity: (f32::from(effect.progress) / 1000.0).clamp(0.0, 1.0),
        pointer: effect.along.push as f32 / scale,
        length: effect.along.span as f32 / scale,
        start: effect.along.start as f32 / scale,
        edge: effect.edge,
        animate: frame.animate,
    };
    let transform = match effect.edge {
        Edge::Top => Transform::from_scale(scale, scale),
        Edge::Bottom => Transform::from_row(scale, 0.0, 0.0, -scale, 0.0, frame.height as f32),
        Edge::Right => Transform::from_row(0.0, scale, -scale, 0.0, frame.width as f32, 0.0),
        _ => Transform::from_row(0.0, scale, scale, 0.0, 0.0, 0.0),
    };
    render(frame, &p, transform, seconds);
}

fn render(frame: &mut Frame, p: &Push, transform: Transform, seconds: f32) {
    frame.pixels.fill(0);
    let state = &mut frame.studio;
    if state.last.is_none_or(|last| seconds < last) {
        state.sparks.reset();
        state.last = None;
    }
    let dt = state
        .last
        .map_or(1.0 / 60.0, |last| (seconds - last).clamp(0.0, 0.05));
    state.last = Some(seconds);
    let Some(pixmap) = PixmapMut::from_bytes(&mut frame.pixels, frame.width, frame.height) else {
        return;
    };
    let mut c = Canvas {
        pixmap,
        paint: Paint {
            blend_mode: BlendMode::Plus,
            ..Paint::default()
        },
        transform,
        look: p.look,
        dark: frame.dark,
    };
    match p.look.style {
        Style::Membrane | Style::Clouds => field::render(&mut c, p, &mut state.field),
        Style::Sparks => sparks::render(&mut c, p, &mut state.sparks, &mut state.random, dt),
        Style::Waves => waves::render(&mut c, p),
        Style::Sunrise => sunrise::render(&mut c, p),
        Style::Aurora => aurora::render(&mut c, p),
        Style::Arcs => arcs::render(&mut c, p, &mut state.random),
        Style::Ferrofluid => ferrofluid::render(&mut c, p),
    }
    for px in frame.pixels.chunks_exact_mut(4) {
        if p.look.style != Style::Ferrofluid {
            let max = px[0].max(px[1]).max(px[2]);
            let alpha = max.min(235);
            if max > alpha {
                for channel in &mut px[..3] {
                    *channel = (u16::from(*channel) * u16::from(alpha) / u16::from(max)) as u8;
                }
            }
            px[3] = alpha;
        }
        for channel in px {
            *channel = (f64::from(*channel) * frame.opacity.clamp(0.0, 1.0)).round() as u8;
        }
    }
}

const RAMP: f32 = 0.12;
const HOLD: f32 = 0.35;
pub(super) fn arrival(frame: &mut Frame, look: Look, elapsed: Duration, duration: Duration) {
    let bloom = if frame.animate {
        BLOOM_DURATION
    } else {
        REDUCED_DURATION
    };
    let arrival = frame.arrival;
    if look.show_edge
        && elapsed < bloom
        && let Some(a) = arrival
        && let Some(edge) = a.edge
    {
        let scale = frame.scale.max(1.0) as f32;
        let seconds = elapsed.as_secs_f32();
        let progress = if frame.animate {
            (seconds / RAMP).min(1.0)
                * (1.0 - (seconds - HOLD).max(0.0) / (BLOOM_DURATION.as_secs_f32() - HOLD)).max(0.0)
        } else {
            1.0 - seconds / bloom.as_secs_f32()
        };
        let (pointer, length, transform) = match edge {
            Edge::Top => (a.center.0, frame.width, Transform::from_scale(scale, scale)),
            Edge::Bottom => (
                a.center.0,
                frame.width,
                Transform::from_row(scale, 0.0, 0.0, -scale, 0.0, frame.height as f32),
            ),
            Edge::Right => (
                a.center.1,
                frame.height,
                Transform::from_row(0.0, scale, -scale, 0.0, frame.width as f32, 0.0),
            ),
            _ => (
                a.center.1,
                frame.height,
                Transform::from_row(0.0, scale, scale, 0.0, 0.0, 0.0),
            ),
        };
        let p = Push {
            look,
            phase: if frame.animate {
                seconds * look.speed * 1.6
            } else {
                0.0
            },
            proximity: progress,
            pointer: pointer / scale,
            length: length as f32 / scale,
            start: 0.0,
            edge,
            animate: frame.animate,
        };
        render(frame, &p, transform, seconds);
        return;
    }
    frame.pixels.fill(0);
    if look.show_marker {
        let offset = if look.show_edge && arrival.is_some_and(|a| a.edge.is_some()) {
            bloom
        } else {
            Duration::ZERO
        };
        raster::render_marker(
            frame,
            look,
            elapsed.saturating_sub(offset),
            duration.saturating_sub(offset),
        );
    }
}
