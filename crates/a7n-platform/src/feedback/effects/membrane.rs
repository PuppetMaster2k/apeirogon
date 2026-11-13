// SPDX-License-Identifier: GPL-3.0-or-later

//! A tensioned line, parting on departure and closing around the arrival.

use std::f32::consts::TAU;

use a7n_proto::v1::Edge;
use tiny_skia::{Path, PathBuilder, Transform};

use super::{Push, ease_out};
use crate::feedback::raster::{Arrival, Canvas, MEMBRANE_ARRIVAL_SIZE, MEMBRANE_DEPTH};
use crate::feedback::timeline::{PLAY_DURATION, SHUDDER_DURATION};

const INSET: f32 = 6.0;
const STROKE: f32 = 2.5;
const HALO: f32 = 8.0;
const HALO_ALPHA: f32 = 0.15;
const DENT_DEPTH: f32 = 60.0;
const DEPTH_PROGRESS: f32 = 0.95;
const SIGMA_WIDE: f32 = 120.0;
const SIGMA_NARROW: f32 = 50.0;
const TIP_MIN_WIDTH: f32 = 0.55;
const STEP: f32 = 2.0;
const SHUDDER_AMPLITUDE: f32 = 0.75;
const SHUDDER_SECONDS: f32 = 0.15;
const SHUDDER_START: f32 = 0.5;
const SHUDDER_END: f32 = 1.5;
const SHUDDER_TIP_SIGMA: f32 = 0.25;
const SHUDDER_SIGMA: f32 = 24.0;
const SHUDDER_WAVELENGTH: f32 = 48.0;
const SPLIT_START: f32 = 4.0;
const SPLIT_END: f32 = 200.0;
const CURL_DEPTH: f32 = 14.0;
const CURL_SIGMA: f32 = 16.0;
const CLOSE_PHASE: f32 = 0.4;
const FADE_PHASE: f32 = 0.7;
const RING_DELAY_SECONDS: f32 = 0.12;
const RING_RADIUS: f32 = 90.0;
const SECOND_RING_STROKE: f32 = 1.5;
const RING_STROKE: f32 = 2.0;
const ARRIVAL_MARGIN: f32 = 0.86;

fn bell(distance: f32, sigma: f32) -> f32 {
    (-0.5 * (distance / sigma).powi(2)).exp()
}

// Fixed raster dimensions are exactly representable in f32.
#[allow(clippy::cast_precision_loss)]
pub(super) fn departure(canvas: &mut Canvas<'_>, push: &Push) {
    let scale = push.depth / MEMBRANE_DEPTH as f32;
    let finish = push.breakthrough();
    let phase = finish.map_or(0.0, ease_out);
    // Delay the retraction before easing out: halfway is ~69 px, the end is 200 px.
    let gap = finish.map_or(0.0, |p| {
        (SPLIT_START + (SPLIT_END - SPLIT_START) * ease_out(p.powi(3))) * scale
    });
    let pressure = (push.progress / DEPTH_PROGRESS).min(1.0);
    let tick_elapsed = (1.0 - push.motion.tick) * SHUDDER_DURATION.as_secs_f32();
    let tick = (1.0 - tick_elapsed / SHUDDER_SECONDS).clamp(0.0, 1.0);
    let sigma = (SIGMA_WIDE + (SIGMA_NARROW - SIGMA_WIDE) * pressure) * scale;
    let dent = DENT_DEPTH * scale * pressure;
    let bend = |x: f32| {
        let distance = x - push.pointer;
        let shudder = if tick > 0.0 && finish.is_none() {
            let wave = distance.abs()
                - sigma * (SHUDDER_START + (1.0 - tick) * (SHUDDER_END - SHUDDER_START));
            // Only the wave is bounded; its value and first two derivatives vanish before the shoulders.
            let window = (1.0 - (distance / (sigma * SHUDDER_END)).powi(2))
                .max(0.0)
                .powi(3)
                * (1.0 - bell(distance, sigma * SHUDDER_TIP_SIGMA));
            SHUDDER_AMPLITUDE
                * scale
                * tick.powi(2)
                * window
                * bell(wave, SHUDDER_SIGMA * scale)
                * (wave / (SHUDDER_WAVELENGTH * scale) * TAU).sin()
        } else {
            0.0
        };
        let curl = if finish.is_some() {
            CURL_DEPTH
                * scale
                * (0.3 + phase * 0.7)
                * bell(distance.abs() - gap, CURL_SIGMA * scale)
        } else {
            0.0
        };
        INSET * scale
            + dent * (1.0 - phase) * bell(distance, sigma)
            + shudder * (1.0 - phase)
            + curl
    };
    for (start, end) in [(0.0, push.pointer - gap), (push.pointer + gap, push.length)] {
        stroke_line(
            canvas,
            (start.max(0.0), end.min(push.length)),
            scale,
            1.0,
            |x| {
                let pressure = pressure * (1.0 - phase) * bell(x - push.pointer, sigma);
                (bend(x), pressure, x / push.length)
            },
        );
    }
}

fn segment(start: f32, end: f32, bend: &impl Fn(f32) -> (f32, f32, f32)) -> Option<Path> {
    let mut path = PathBuilder::new();
    let third = (end - start) / 3.0;
    let slope = |x: f32| bend(x + 0.5).0 - bend(x - 0.5).0;
    path.move_to(start, bend(start).0);
    path.cubic_to(
        start + third,
        bend(start).0 + slope(start) * third,
        end - third,
        bend(end).0 - slope(end) * third,
        end,
        bend(end).0,
    );
    path.finish()
}

fn stroke_line(
    canvas: &mut Canvas<'_>,
    (start, end): (f32, f32),
    scale: f32,
    opacity: f32,
    bend: impl Fn(f32) -> (f32, f32, f32),
) {
    // Layer the halo under the whole line before drawing the tapered core.
    for (width, alpha) in [
        (HALO, HALO_ALPHA / 3.0),
        (HALO * 0.75, HALO_ALPHA / 3.0),
        (HALO * 0.5, HALO_ALPHA / 3.0),
        (STROKE, 1.0),
    ] {
        let mut x = start;
        while x < end {
            let next = (x + STEP * scale).min(end);
            let (_, pressure, position) = bend(x.midpoint(next));
            if let Some(path) = segment(x, next, &bend) {
                canvas.membrane_color(position, pressure, alpha * opacity);
                canvas.stroke(
                    &path,
                    width * scale * (1.0 - (1.0 - TIP_MIN_WIDTH) * pressure.clamp(0.0, 1.0)),
                );
            }
            x = next;
        }
    }
}

// Fixed raster dimensions are exactly representable in f32.
#[allow(clippy::cast_precision_loss)]
pub(super) fn arrival(
    canvas: &mut Canvas<'_>,
    arrival: Arrival,
    radius: f32,
    motion: f32,
    phase: f32,
) {
    let scale = radius / (MEMBRANE_ARRIVAL_SIZE as f32 / 2.0 * ARRIVAL_MARGIN);
    let alpha = 1.0 - ((phase - FADE_PHASE) / (1.0 - FADE_PHASE)).clamp(0.0, 1.0);
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
        INSET * scale
    } else {
        inward
    };
    let gap = radius * (1.0 - ease_out((motion / CLOSE_PHASE).min(1.0)));
    for (start, end) in [(0.0, along - gap), (along + gap, length)] {
        stroke_line(
            canvas,
            (start.max(0.0), end.min(length)),
            scale,
            alpha,
            |x| (inset, 0.0, x / length),
        );
    }
    canvas.transform = Transform::identity();
    for (delay, stroke) in [
        (0.0, RING_STROKE),
        (
            RING_DELAY_SECONDS / PLAY_DURATION.as_secs_f32(),
            SECOND_RING_STROKE,
        ),
    ] {
        let age = motion - delay;
        if age <= 0.0 {
            continue;
        }
        let radius = RING_RADIUS * scale * ease_out(age);
        for (width, opacity) in [
            (HALO, HALO_ALPHA / 2.0),
            (HALO * 0.6, HALO_ALPHA / 2.0),
            (stroke, 1.0),
        ] {
            canvas.ring(center, radius, width * scale, opacity * alpha);
        }
    }
}
