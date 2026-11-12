// SPDX-License-Identifier: GPL-3.0-or-later

//! Paired pressure/arrival shapes in a local coordinate system: x along the edge,
//! y into the display. Every curve is drawn by the shared tiny-skia canvas.

use std::f32::consts::{PI, TAU};

use a7n_proto::v1::Edge;
use tiny_skia::{PathBuilder, Transform};

use super::Style;
use super::raster::{Arrival, Canvas};
use super::timeline::{End, Motion};

const EDGE_INSET: f32 = 0.09;
const LINE_WIDTH: f32 = 0.045;
const DENT_DEPTH: f32 = 0.72;
const BELL_WIDE: f32 = 2.8;
const BELL_NARROW: f32 = 1.0;
const SHUDDER_REACH: f32 = 6.0;
const PARTING_REACH: f32 = 5.0;
const PATH_STEP: f32 = 0.12;
const PARTICLES: u16 = 32;
const GOLDEN_ANGLE: f32 = 2.399_963_1;
const PARTICLE_VARIATION: f32 = 12.989_8;
const PARTICLE_RADIUS: f32 = 0.035;
const GATHER_SPREAD: f32 = 2.6;
const GATHER_SPEED: f32 = 1.8;
const APERTURE_SPAN: f32 = 3.2;
const APERTURE_DEPTH: f32 = 0.34;
const ARRIVAL_MARGIN: f32 = 0.86;
const STATIC_PHASE: f32 = 0.4;

pub(super) struct Push {
    pub style: Style,
    pub length: f32,
    pub depth: f32,
    pub pointer: f32,
    pub progress: f32,
    pub motion: Motion,
}

impl Push {
    fn breakthrough(&self) -> Option<f32> {
        self.motion
            .finish
            .and_then(|(end, phase)| matches!(end, End::Crossed).then_some(phase))
    }
}

pub(super) fn departure(canvas: &mut Canvas<'_>, push: &Push) {
    match push.style {
        Style::Membrane => membrane(canvas, push),
        Style::Sparks => gathering(canvas, push),
        Style::Aperture => aperture(canvas, push),
        Style::Glow | Style::Beam => {}
    }
}

fn ease_out(phase: f32) -> f32 {
    1.0 - (1.0 - phase).powi(3)
}
fn bell(distance: f32, width: f32) -> f32 {
    (-0.5 * (distance / width).powi(2)).exp()
}

fn membrane(canvas: &mut Canvas<'_>, push: &Push) {
    let phase = push.breakthrough().map_or(0.0, ease_out);
    let gap = phase * (push.depth * PARTING_REACH).min(push.length * 0.42);
    let width = push.depth * LINE_WIDTH * (1.0 - push.progress * 0.65);
    canvas.linear(0.0, push.length, push.progress * 0.35, 1.0);
    for (start, end) in [(0.0, push.pointer - gap), (push.pointer + gap, push.length)] {
        if end <= start {
            continue;
        }
        let bend = |x: f32| {
            let distance = x - push.pointer;
            let sigma = push.depth * (BELL_WIDE + (BELL_NARROW - BELL_WIDE) * push.progress);
            let dent = push.depth * DENT_DEPTH * push.progress.powf(1.5) * (1.0 - phase);
            let wave_distance =
                distance.abs() - (1.0 - push.motion.tick) * push.depth * SHUDDER_REACH;
            let shudder = push.motion.tick
                * push.depth
                * LINE_WIDTH
                * bell(wave_distance, push.depth * 0.5)
                * (wave_distance / push.depth * TAU).sin();
            let curl = phase * push.depth * 0.3 * bell(distance.abs() - gap, push.depth * 0.5);
            push.depth * EDGE_INSET + dent * bell(distance, sigma) + shudder + curl
        };
        let path = curved_line(
            start.max(0.0),
            end.min(push.length),
            push.depth * PATH_STEP,
            bend,
        );
        if let Some(path) = path {
            canvas.stroke(&path, width);
        }
    }
}

/// Hermite segments retain the bell's tangent, including where the line parts.
fn curved_line(
    start: f32,
    end: f32,
    step: f32,
    bend: impl Fn(f32) -> f32,
) -> Option<tiny_skia::Path> {
    let mut path = PathBuilder::new();
    let mut x = start;
    path.move_to(x, bend(x));
    while x < end {
        let next = (x + step.max(1.0)).min(end);
        let third = (next - x) / 3.0;
        let slope = |at: f32| bend(at + 0.5) - bend(at - 0.5);
        path.cubic_to(
            x + third,
            bend(x) + slope(x) * third,
            next - third,
            bend(next) - slope(next) * third,
            next,
            bend(next),
        );
        x = next;
    }
    path.finish()
}

fn gathering(canvas: &mut Canvas<'_>, push: &Push) {
    let contraction = push
        .breakthrough()
        .map_or(1.0, |p| (1.0 - ease_out(p)).powi(2));
    let velocity = 0.5 + push.progress * GATHER_SPEED;
    for particle in 0..PARTICLES {
        if f32::from(particle) > 3.0 + push.progress * f32::from(PARTICLES - 3) {
            break;
        }
        let seed = f32::from(particle);
        let age = (push.motion.seconds * velocity + seed / f32::from(PARTICLES)).fract();
        let angle = seed * GOLDEN_ANGLE + age * TAU;
        let radius = (1.0 - age) * push.depth * contraction;
        let center = (
            push.pointer + angle.cos() * radius * GATHER_SPREAD,
            push.depth * EDGE_INSET + angle.sin().abs() * radius * 0.8,
        );
        let size = push.depth * PARTICLE_RADIUS * (1.0 + age + push.motion.tick);
        let opacity = (1.0 - age).min(age * 8.0) * (0.6 + push.progress * 0.4);
        canvas.soft(center, size * 3.0, seed / f32::from(PARTICLES), opacity);
        canvas.circle(center, size * 3.0, None);
        canvas.paint.shader = canvas.solid(seed / f32::from(PARTICLES), opacity);
        canvas.circle(center, size * 0.6, None);
    }
}

fn lens(canvas: &mut Canvas<'_>, center: (f32, f32), half: f32, depth: f32, alpha: f32) {
    if half <= 0.0 || depth <= 0.0 {
        return;
    }
    let (x, y) = center;
    let mut path = PathBuilder::new();
    path.move_to(x - half, y);
    path.cubic_to(
        x - half * 0.3,
        y - depth,
        x + half * 0.3,
        y - depth,
        x + half,
        y,
    );
    path.cubic_to(
        x + half * 0.3,
        y + depth,
        x - half * 0.3,
        y + depth,
        x - half,
        y,
    );
    path.close();
    if let Some(path) = path.finish() {
        canvas.soft(center, half, 0.0, 0.75 * alpha);
        canvas.fill(&path);
        canvas.linear(x - half, x + half, 0.0, 0.9 * alpha);
        canvas.stroke(&path, (depth * 0.12).max(0.6));
    }
}

fn aperture(canvas: &mut Canvas<'_>, push: &Push) {
    let phase = push.breakthrough();
    let opening = phase.map_or(push.progress, |p| 1.0 - ease_out(p));
    let half = push.depth * (0.15 + opening * APERTURE_SPAN);
    let depth = push.depth * APERTURE_DEPTH * (opening + push.motion.tick * 0.1);
    let center = (push.pointer, push.depth * 0.4);
    lens(canvas, center, half, depth, 1.0);
    if let Some(phase) = phase {
        let flash = (1.0 - phase * 5.0).max(0.0);
        canvas.soft(center, push.depth * 0.6, push.progress, flash);
        canvas.circle(center, push.depth * 0.6, None);
    }
}

pub(super) fn arrival(
    canvas: &mut Canvas<'_>,
    style: Style,
    arrival: Arrival,
    phase: f32,
    animate: bool,
) {
    let motion = if animate { phase } else { STATIC_PHASE };
    let radius = canvas.width.min(canvas.height) / 2.0 * ARRIVAL_MARGIN;
    match style {
        Style::Membrane => arriving_membrane(canvas, arrival, radius, motion, 1.0 - phase),
        Style::Sparks => burst(canvas, arrival.center, radius, motion, 1.0 - phase),
        Style::Aperture => {
            let opening = (PI * motion).sin().max(0.0);
            lens(
                canvas,
                arrival.center,
                radius * opening,
                radius * APERTURE_DEPTH * opening,
                1.0 - phase,
            );
        }
        Style::Glow | Style::Beam => {}
    }
}

fn arriving_membrane(
    canvas: &mut Canvas<'_>,
    arrival: Arrival,
    radius: f32,
    phase: f32,
    alpha: f32,
) {
    let center = arrival.center;
    let edge = arrival.edge.unwrap_or(Edge::Left);
    let (length, _) = canvas.orient(edge);
    let (along, inward) = match edge {
        Edge::Top => (center.0, center.1),
        Edge::Bottom => (center.0, canvas.height - center.1),
        Edge::Right => (center.1, canvas.width - center.0),
        Edge::Left | Edge::Unspecified => (center.1, center.0),
    };
    let inset = if arrival.edge.is_some() {
        radius * EDGE_INSET
    } else {
        inward
    };
    let gap = radius * (1.0 - ease_out((phase * 2.0).min(1.0)));
    for (start, end) in [(0.0, along - gap), (along + gap, length)] {
        if start >= end {
            continue;
        }
        let mut path = PathBuilder::new();
        path.move_to(start, inset);
        path.line_to(end, inset);
        if let Some(path) = path.finish() {
            canvas.soft((along, inset), radius, phase, alpha);
            canvas.stroke(&path, radius * LINE_WIDTH);
        }
    }
    canvas.transform = Transform::identity();
    for delay in [0.0, 0.18] {
        let age = (phase - delay) / (1.0 - delay);
        if age > 0.0 {
            canvas.ring(
                center,
                radius * ease_out(age),
                radius * LINE_WIDTH * (1.0 - age * 0.5),
                alpha * (1.0 - age).powi(2),
            );
        }
    }
}

fn burst(canvas: &mut Canvas<'_>, center: (f32, f32), radius: f32, phase: f32, alpha: f32) {
    for particle in 0..PARTICLES {
        let seed = f32::from(particle);
        let angle = seed * GOLDEN_ANGLE;
        let velocity = 0.4 + 0.6 * ((seed + 1.0) * PARTICLE_VARIATION).sin().abs();
        let distance = radius * velocity * ease_out(phase);
        let point = (
            center.0 + angle.cos() * distance,
            center.1 + angle.sin() * distance,
        );
        let size = radius * PARTICLE_RADIUS * (1.0 - phase * 0.7);
        canvas.soft(point, size * 3.0, seed / f32::from(PARTICLES), alpha);
        canvas.circle(point, size * 3.0, None);
        canvas.paint.shader = canvas.solid(seed / f32::from(PARTICLES), alpha);
        canvas.circle(point, size * 0.6, None);
    }
}
