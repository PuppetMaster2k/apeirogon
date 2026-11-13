// SPDX-License-Identifier: GPL-3.0-or-later

//! Half-resolution plasma field, composited into the native overlay with normal alpha.

use std::time::Duration;

use a7n_proto::v1::Edge;
use tiny_skia::{ColorU8, FilterQuality, Pixmap, PixmapMut, PixmapPaint, Transform};

use super::Look;
use super::raster::{Arrival, EdgeEffect, Frame};
use super::timeline::{End, Motion, SHUDDER_DURATION};

pub(super) const OVERLAY_DEPTH: u32 = 72;
pub(super) const ARRIVAL_DIAMETER: u32 = 604;
pub(super) const FINISH_DURATION: Duration = Duration::from_millis(500);
pub(super) const ARRIVAL_DURATION: Duration = Duration::from_millis(700);
const DEPTH_START: f32 = 30.0;
const DEPTH_END: f32 = 70.0;
const RADIUS_START: f32 = 90.0;
const RADIUS_END: f32 = 260.0;
const RADIUS_PROGRESS_START: f32 = 0.1;
const ARRIVAL_START: f32 = 60.0;
const ARRIVAL_END: f32 = 300.0;
const FADE_START: f32 = 0.5;
const PULSE_SECONDS: f32 = 0.12;
const TICK_GAIN: f32 = 0.3;
const FLASH_GAIN: f32 = 0.4;
const PROGRESS_PEAK: f32 = 0.95;
const MAX_ALPHA: f32 = 0.75;
const WAVE_CONTRAST: f32 = 1.7;
const SPATIAL_FREQUENCY: f32 = 0.0225;
const DEPTH_DECAY: f32 = 3.0;
const WAVE_INTENSITY_BASE: f32 = 0.7;
const WAVE_INTENSITY_GAIN: f32 = 0.3;
const CORE_WIDTH: f32 = 2.5;
const CORE_GAIN: f32 = 0.2;
const CORE_TINT: f32 = 0.55;
const PHASE_SPEED: f32 = 2.2;
const POSITION_RATE: f32 = 14.0;
const VISIBILITY_RATE: f32 = 6.0;
const MAX_STEP: f32 = 0.05;
const STATIC_SECONDS: f32 = 0.4;
const LUT_SIZE: usize = 256;
const UPSCALE: u32 = 2;

#[derive(Default)]
pub(super) struct State {
    low: Option<Pixmap>,
    smoothed: Option<(f32, f32, f32)>,
}

impl State {
    fn smooth(&mut self, seconds: f32, pointer: f32, gain: f32, animate: bool) -> (f32, f32) {
        let (pointer, gain) = self
            .smoothed
            .filter(|last| animate && last.0 <= seconds)
            .map_or((pointer, gain), |(last, along, active)| {
                let dt = (seconds - last).min(MAX_STEP);
                (
                    along + (pointer - along) * (dt * POSITION_RATE).min(1.0),
                    active + (gain - active) * (dt * VISIBILITY_RATE).min(1.0),
                )
            });
        self.smoothed = Some((seconds, pointer, gain));
        (pointer, gain)
    }
}

struct Field {
    edge: Edge,
    point: (f32, f32),
    scale: f32,
    radius: f32,
    depth: f32,
    gain: f32,
    seconds: f32,
}

// Monitor geometry and raster dimensions fit exactly enough in Skia's f32 coordinates.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub(super) fn departure(frame: &mut Frame, effect: EdgeEffect, motion: Motion) {
    let horizontal = matches!(effect.edge, Edge::Top | Edge::Bottom);
    let scale = if horizontal {
        frame.height
    } else {
        frame.width
    } as f32
        / OVERLAY_DEPTH as f32;
    let progress = (f32::from(effect.progress) / 1000.0 / PROGRESS_PEAK).min(1.0);
    let (pointer, mut gain) = frame.flow.smooth(
        motion.seconds,
        (effect.along.start + effect.along.push) as f32,
        progress,
        frame.animate,
    );
    let pointer = pointer - effect.along.start as f32;
    let extent = ((progress * PROGRESS_PEAK - RADIUS_PROGRESS_START)
        / (PROGRESS_PEAK - RADIUS_PROGRESS_START))
        .clamp(0.0, 1.0);
    let mut radius = (RADIUS_START + (RADIUS_END - RADIUS_START) * extent) * scale;
    let mut depth = (DEPTH_START + (DEPTH_END - DEPTH_START) * extent) * scale;
    let tick_elapsed = (1.0 - motion.tick) * SHUDDER_DURATION.as_secs_f32();
    gain *= 1.0 + TICK_GAIN * (1.0 - tick_elapsed / PULSE_SECONDS).clamp(0.0, 1.0);
    if let Some((End::Crossed, phase)) = motion.finish {
        radius *= (1.0 - phase).powi(2);
        depth *= (1.0 - phase).powi(2);
        let elapsed = phase * FINISH_DURATION.as_secs_f32();
        gain *= 1.0 + FLASH_GAIN * (1.0 - elapsed / PULSE_SECONDS).max(0.0);
    }
    let point = match effect.edge {
        Edge::Top => (pointer, 0.0),
        Edge::Bottom => (pointer, frame.height.saturating_sub(1) as f32),
        Edge::Right => (frame.width.saturating_sub(1) as f32, pointer),
        Edge::Left | Edge::Unspecified => (0.0, pointer),
    };
    render(
        frame,
        effect.look,
        &Field {
            edge: effect.edge,
            point,
            scale,
            radius,
            depth,
            gain,
            seconds: if frame.animate {
                motion.seconds
            } else {
                STATIC_SECONDS
            },
        },
    );
}

#[allow(clippy::cast_precision_loss)] // Fixed overlay dimensions fit f32 geometry.
pub(super) fn arrival(frame: &mut Frame, look: Look, elapsed: Duration, duration: Duration) {
    let phase = (elapsed.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0);
    let motion = if frame.animate {
        1.0 - (1.0 - phase).powi(3)
    } else {
        STATIC_SECONDS
    };
    let scale = frame.width.min(frame.height) as f32 / ARRIVAL_DIAMETER as f32;
    let arrival = frame.arrival.unwrap_or(Arrival {
        center: (frame.width as f32 / 2.0, frame.height as f32 / 2.0),
        edge: None,
    });
    let gain = if frame.animate {
        1.0 - ((phase - FADE_START) / (1.0 - FADE_START)).clamp(0.0, 1.0)
    } else {
        1.0 - phase
    };
    render(
        frame,
        look,
        &Field {
            edge: arrival.edge.unwrap_or(Edge::Left),
            point: arrival.center,
            scale,
            radius: (ARRIVAL_START + (ARRIVAL_END - ARRIVAL_START) * motion) * scale,
            depth: DEPTH_END * scale,
            gain,
            seconds: if frame.animate {
                elapsed.as_secs_f32()
            } else {
                STATIC_SECONDS
            },
        },
    );
}

// Fixed-size LUT indices and bounded raster coordinates are safe to round to pixels.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn render(frame: &mut Frame, look: Look, field: &Field) {
    frame.pixels.fill(0);
    let width = frame.width.div_ceil(UPSCALE);
    let height = frame.height.div_ceil(UPSCALE);
    if !frame
        .flow
        .low
        .as_ref()
        .is_some_and(|p| p.width() == width && p.height() == height)
    {
        frame.flow.low = Pixmap::new(width, height);
    }
    let Some(low) = frame.flow.low.as_mut() else {
        return;
    };
    let lut: [[u8; 3]; LUT_SIZE] = std::array::from_fn(|i| {
        look.color
            .sample(i as f32 / (LUT_SIZE - 1) as f32, frame.dark)
    });
    for (index, pixel) in low.pixels_mut().iter_mut().enumerate() {
        let x = (index % width as usize) as f32 * UPSCALE as f32;
        let y = (index / width as usize) as f32 * UPSCALE as f32;
        let (wave, intensity, core) =
            sample(field, (x, y), (frame.width as f32, frame.height as f32));
        let [red, green, blue] = lut[(wave * (LUT_SIZE - 1) as f32) as usize].map(|channel| {
            let channel = f32::from(channel);
            (channel + (255.0 - channel) * core * CORE_TINT).round() as u8
        });
        let alpha = (intensity * frame.opacity as f32).clamp(0.0, MAX_ALPHA);
        *pixel = ColorU8::from_rgba(red, green, blue, (alpha * 255.0).round() as u8).premultiply();
    }
    if let Some(mut target) = PixmapMut::from_bytes(&mut frame.pixels, frame.width, frame.height) {
        target.draw_pixmap(
            0,
            0,
            low.as_ref(),
            &PixmapPaint {
                quality: FilterQuality::Bilinear,
                ..PixmapPaint::default()
            },
            Transform::from_scale(UPSCALE as f32, UPSCALE as f32),
            None,
        );
    }
}

fn smoothstep(value: f32) -> f32 {
    value.powi(2) * (3.0 - 2.0 * value)
}

fn sample(field: &Field, (x, y): (f32, f32), (width, height): (f32, f32)) -> (f32, f32, f32) {
    let dx = (x - field.point.0) / field.scale / 2.0;
    let dy = (y - field.point.1) / field.scale / 2.0;
    let distance = match field.edge {
        Edge::Top => y,
        Edge::Bottom => height - 1.0 - y,
        Edge::Right => width - 1.0 - x,
        Edge::Left | Edge::Unspecified => x,
    };
    let along = if matches!(field.edge, Edge::Top | Edge::Bottom) {
        x - field.point.0
    } else {
        y - field.point.1
    };
    // Independent axes keep the band soft without the circular mask pinching its shoulders.
    let edge = (1.0 - distance / field.depth.max(f32::EPSILON)).clamp(0.0, 1.0);
    let mouse = (1.0 - along.abs() / field.radius.max(f32::EPSILON)).clamp(0.0, 1.0);
    if edge == 0.0 || mouse == 0.0 || field.gain == 0.0 {
        return (0.0, 0.0, 0.0);
    }
    let nx = x / field.scale / 2.0 * SPATIAL_FREQUENCY;
    let ny = y / field.scale / 2.0 * SPATIAL_FREQUENCY;
    let phase = field.seconds * PHASE_SPEED;
    let wave = ((nx * 2.4 + phase + dx * 0.03).sin()
        + (ny * 2.4 - phase * 0.9 + dy * 0.03).cos()
        + ((nx + ny) * 1.8 - phase * 1.4).sin())
        / 6.0
        + 0.5;
    let wave = ((wave - 0.5) * WAVE_CONTRAST + 0.5).clamp(0.0, 1.0);
    let tail = (-DEPTH_DECAY * (1.0 - edge)).exp() * smoothstep(edge);
    let core = (-(distance / (CORE_WIDTH * field.scale)).powi(2)).exp();
    let intensity = (tail * (WAVE_INTENSITY_BASE + WAVE_INTENSITY_GAIN * wave) + core * CORE_GAIN)
        * smoothstep(mouse)
        * field.gain;
    (wave, intensity, core)
}
