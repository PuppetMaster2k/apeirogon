// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared premultiplied RGBA frames; native backends only place and blit them.

use std::time::Duration;

use a7n_proto::v1::Edge;

use super::{Landing, Look, Style};

const CHANNELS: usize = 4;
const BYTE_MAX: f64 = 255.0;

#[derive(Default)]
pub(super) struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub animate: bool,
    pub dark: bool,
    pub opacity: f64,
    pub stroke: f64,
}

impl Frame {
    pub fn resize(&mut self, width: u32, height: u32) {
        if (self.width, self.height) != (width, height) {
            self.width = width;
            self.height = height;
            self.pixels
                .resize(width as usize * height as usize * CHANNELS, 0);
        }
        self.pixels.fill(0);
    }
}

/// Positions along the whole pushed edge, in the same units as the frame.
#[derive(Clone, Copy)]
pub(super) struct Along {
    pub start: f64,
    pub span: f64,
    pub push: f64,
}

/// The last absolute edge command, reused for timer frames without querying the OS.
#[derive(Clone, Copy)]
pub(super) struct EdgeEffect {
    pub look: Look,
    pub edge: Edge,
    pub progress: u16,
    pub along: Along,
}

impl EdgeEffect {
    pub fn render(self, frame: &mut Frame, time: Duration) {
        render_edge(frame, self.look, self.edge, self.progress, time, self.along);
    }
}

const PROGRESS_FULL: f64 = 1000.0;
pub(super) const EDGE_DEPTH: u32 = 48;
const APERTURE_MIN_SPAN: f64 = 0.03;
const APERTURE_PULSE: f64 = 0.05;
const RIPPLE_PERIOD: f64 = 90.0;
const RIPPLE_SPEED: f64 = 160.0;
const STATIC_TIME: f64 = 0.4;

// Coverage and colour channels are clamped before conversion to bytes.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn pixel(target: &mut [u8], color: [u8; 3], alpha: f64) {
    let alpha = alpha.clamp(0.0, 1.0);
    for (target, value) in target.iter_mut().zip(color) {
        *target = (f64::from(value) * alpha).round() as u8;
    }
    target[3] = (BYTE_MAX * alpha).round() as u8;
}

pub(super) fn render_edge(
    frame: &mut Frame,
    look: Look,
    edge: Edge,
    progress: u16,
    time: Duration,
    along: Along,
) {
    frame.pixels.fill(0);
    let progress = (f64::from(progress) / PROGRESS_FULL).min(1.0);
    let seconds = if frame.animate {
        time.as_secs_f64()
    } else {
        STATIC_TIME
    };
    if look.style == Style::Sparks {
        render_sparks(frame, look, edge, progress, seconds, along);
        return;
    }
    let horizontal = matches!(edge, Edge::Top | Edge::Bottom);
    let thickness = f64::from(if horizontal {
        frame.height
    } else {
        frame.width
    });
    for y in 0..frame.height {
        for x in 0..frame.width {
            let position = f64::from(if horizontal { x } else { y });
            // Desktop spans are bounded, so conversion to the sampler's f32 is safe.
            #[allow(clippy::cast_possible_truncation)]
            let t = ((along.start + position) / along.span.max(1.0)) as f32;
            let depth = depth(edge, x, y, frame.width, frame.height);
            let coverage = edge_coverage(
                look.style,
                position - along.push,
                depth,
                thickness,
                along.span,
                progress,
                seconds,
            );
            let offset = (y as usize * frame.width as usize + x as usize) * CHANNELS;
            pixel(
                &mut frame.pixels[offset..offset + CHANNELS],
                look.color.sample(t, frame.dark),
                frame.opacity * coverage,
            );
        }
    }
}

pub(super) fn render_landing(frame: &mut Frame, look: Look, elapsed: Duration, duration: Duration) {
    let (radius, alpha) = look.landing.frame(elapsed, duration, frame.animate);
    let radius = f64::from(frame.width.min(frame.height)) / 2.0 * radius;
    let inner = if look.landing == Landing::Dot {
        0.0
    } else {
        (radius - frame.stroke).max(0.0)
    };
    for y in 0..frame.height {
        for x in 0..frame.width {
            let dx = f64::from(x) + 0.5 - f64::from(frame.width) / 2.0;
            let dy = f64::from(y) + 0.5 - f64::from(frame.height) / 2.0;
            let distance = dx.hypot(dy);
            let coverage = (radius - distance + 0.5).clamp(0.0, 1.0)
                * if inner == 0.0 {
                    1.0
                } else {
                    (distance - inner + 0.5).clamp(0.0, 1.0)
                };
            let offset = (y as usize * frame.width as usize + x as usize) * CHANNELS;
            pixel(
                &mut frame.pixels[offset..offset + CHANNELS],
                look.color.sample(
                    landing_position(look.landing, dx, dy, distance, radius),
                    frame.dark,
                ),
                coverage * alpha * frame.opacity,
            );
        }
    }
}

// Angles and normalized radial distances are bounded before sampling.
#[allow(clippy::cast_possible_truncation)]
fn landing_position(style: Landing, dx: f64, dy: f64, distance: f64, radius: f64) -> f32 {
    if style == Landing::Dot {
        (distance / radius.max(1.0)) as f32
    } else {
        ((dy.atan2(dx) + std::f64::consts::PI) / std::f64::consts::TAU) as f32
    }
}

fn depth(edge: Edge, x: u32, y: u32, width: u32, height: u32) -> f64 {
    f64::from(match edge {
        Edge::Top => y,
        Edge::Bottom => height - 1 - y,
        Edge::Left | Edge::Unspecified => x,
        Edge::Right => width - 1 - x,
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "pure coverage math for one pixel"
)]
fn edge_coverage(
    style: Style,
    distance: f64,
    depth: f64,
    thickness: f64,
    span: f64,
    progress: f64,
    time: f64,
) -> f64 {
    let fade = (1.0 - depth / thickness.max(1.0)).powi(2);
    match style {
        Style::Glow | Style::Beam => 1.0,
        Style::Aperture => {
            let half = span * (APERTURE_MIN_SPAN + progress) / 2.0;
            let breath = 1.0 + APERTURE_PULSE * (time * std::f64::consts::TAU).sin();
            ((half * breath - distance.abs()) / thickness.max(1.0)).clamp(0.0, 1.0) * fade
        }
        Style::Ripple => {
            let wave = ((distance.abs() - time * RIPPLE_SPEED) / RIPPLE_PERIOD
                * std::f64::consts::TAU)
                .cos()
                .max(0.0)
                .powi(6);
            let reach = span * (APERTURE_MIN_SPAN + progress);
            wave * (1.0 - distance.abs() / reach.max(1.0)).clamp(0.0, 1.0) * fade
        }
        Style::Sparks => 0.0,
    }
}

fn render_sparks(
    frame: &mut Frame,
    look: Look,
    edge: Edge,
    progress: f64,
    time: f64,
    along: Along,
) {
    const PARTICLES: u32 = 24;
    const LIFETIME: f64 = 0.8;
    const PHASE_STEP: f64 = 0.618_033_988_75;
    const SPREAD: f64 = 100.0;
    const RADIUS: f64 = 1.5;
    let horizontal = matches!(edge, Edge::Top | Edge::Bottom);
    let scale = f64::from(if horizontal {
        frame.height
    } else {
        frame.width
    }) / f64::from(EDGE_DEPTH);
    for particle in 0..PARTICLES {
        if f64::from(particle) >= 2.0 + progress * f64::from(PARTICLES - 2) {
            break;
        }
        let phase = time / LIFETIME + f64::from(particle) * PHASE_STEP;
        let age = phase.fract();
        // Each birth has a stable direction; a timer frame never calls an RNG.
        let direction =
            ((f64::from(particle) + phase.floor()) * PHASE_STEP * std::f64::consts::TAU).sin();
        let along_position = along.push + direction * age * SPREAD * scale;
        let inward = age * f64::from(EDGE_DEPTH) * scale;
        let (x, y) = match edge {
            Edge::Top => (along_position, inward),
            Edge::Bottom => (along_position, f64::from(frame.height) - 1.0 - inward),
            Edge::Left | Edge::Unspecified => (inward, along_position),
            Edge::Right => (f64::from(frame.width) - 1.0 - inward, along_position),
        };
        spark(
            frame,
            look,
            along,
            (x, y),
            along_position,
            RADIUS * scale,
            1.0 - age,
        );
    }
}

// Particle bounds are clipped to the frame before conversion to pixel indexes.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn spark(
    frame: &mut Frame,
    look: Look,
    along: Along,
    center: (f64, f64),
    position: f64,
    radius: f64,
    alpha: f64,
) {
    let color = look.color.sample(
        ((along.start + position) / along.span.max(1.0)) as f32,
        frame.dark,
    );
    let left = (center.0 - radius).max(0.0) as u32;
    let top = (center.1 - radius).max(0.0) as u32;
    let right = (center.0 + radius + 1.0).clamp(0.0, f64::from(frame.width)) as u32;
    let bottom = (center.1 + radius + 1.0).clamp(0.0, f64::from(frame.height)) as u32;
    for y in top..bottom {
        for x in left..right {
            let coverage = (radius - (f64::from(x) - center.0).hypot(f64::from(y) - center.1)
                + 0.5)
                .clamp(0.0, 1.0);
            let offset = (y as usize * frame.width as usize + x as usize) * CHANNELS;
            let target = &mut frame.pixels[offset..offset + CHANNELS];
            let opacity = alpha * coverage * frame.opacity;
            if opacity * BYTE_MAX > f64::from(target[3]) {
                pixel(target, color, opacity);
            }
        }
    }
}
