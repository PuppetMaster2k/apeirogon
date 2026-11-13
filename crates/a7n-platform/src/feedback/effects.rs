// SPDX-License-Identifier: GPL-3.0-or-later

//! Paired pressure/arrival shapes in a local coordinate system: x along the edge,
//! y into the display. Every curve is drawn by the shared tiny-skia canvas.

use std::f32::consts::{PI, TAU};

use tiny_skia::PathBuilder;

use super::Style;
use super::raster::{Arrival, Canvas};
use super::timeline::{End, Motion};

mod membrane;

const EDGE_INSET: f32 = 0.09;
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
        Style::Membrane => membrane::departure(canvas, push),
        Style::Sparks => gathering(canvas, push),
        Style::Aperture => aperture(canvas, push),
        Style::Glow | Style::Beam => {}
    }
}

fn ease_out(phase: f32) -> f32 {
    1.0 - (1.0 - phase).powi(3)
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
        Style::Membrane => membrane::arrival(canvas, arrival, radius, motion, phase),
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
