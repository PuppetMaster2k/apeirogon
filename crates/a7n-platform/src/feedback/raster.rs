// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared premultiplied RGBA frames; native backends only place and blit them.

use std::time::Duration;

use a7n_proto::v1::Edge;

use tiny_skia::{Mask, PixmapMut};

use super::{Landing, Look};

const CHANNELS: usize = 4;
#[derive(Default)]
pub(super) struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub animate: bool,
    pub dark: bool,
    pub opacity: f64,
    pub arrival: Option<Arrival>,
    pub(super) studio: super::effects::State,
    pub scale: f64,
    pub arrival_texture: Vec<u8>,
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
    pub origin: (f64, f64),
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

// Sunrise corona and the widest curtains determine the maximum overlay extent.
pub(super) const OVERLAY_DEPTH: u32 = 540;

#[derive(Clone, Copy)]
pub(super) struct Arrival {
    pub center: (f32, f32),
}

impl Arrival {
    // Native geometry is bounded by the monitor before conversion to raster coordinates.
    #[allow(clippy::cast_possible_truncation)]
    pub fn at(rect: super::look::Rect, point: (f64, f64), scale: f64) -> Self {
        Self {
            center: (
                ((point.0 - rect.x) * scale) as f32,
                ((point.1 - rect.y) * scale) as f32,
            ),
        }
    }
}

pub(super) fn render_departure(frame: &mut Frame, effect: EdgeEffect, seconds: f32) {
    super::effects::departure(frame, effect, seconds);
}

pub(super) fn render_landing(frame: &mut Frame, look: Look, elapsed: Duration, duration: Duration) {
    super::effects::arrival(frame, look, elapsed, duration);
}

// Radial coverage gives both sides of the annulus the same soft transition.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
pub(super) fn mask_landing(frame: &mut Frame, look: Look, elapsed: Duration, duration: Duration) {
    let (radius, alpha) = look.landing.frame(elapsed, duration, frame.animate);
    let scale = look.size.multiplier() * frame.scale.max(1.0);
    let outer = look.landing.radius() * radius * scale;
    let inner = (outer - look.landing.band() * scale).max(0.0);
    let soft = super::look::LANDING_SOFT_EDGE * scale;
    let center = frame.arrival.map_or(
        (f64::from(frame.width) / 2.0, f64::from(frame.height) / 2.0),
        |a| (f64::from(a.center.0), f64::from(a.center.1)),
    );
    let Some(mut mask) = Mask::new(frame.width, frame.height) else {
        frame.pixels.fill(0);
        return;
    };
    let alpha = if frame.animate {
        alpha
    } else {
        1.0 - (elapsed.as_secs_f64() / duration.as_secs_f64()).clamp(0.0, 1.0)
    };
    for (index, value) in mask.data_mut().iter_mut().enumerate() {
        let x = (index % frame.width as usize) as f64 + 0.5 - center.0;
        let y = (index / frame.width as usize) as f64 + 0.5 - center.1;
        let distance = x.hypot(y);
        let edge = if look.landing == Landing::Dot || inner == 0.0 {
            outer - distance
        } else {
            (outer - distance).min(distance - inner)
        };
        let coverage = (edge / soft + 0.5).clamp(0.0, 1.0);
        let coverage = coverage * coverage * (3.0 - 2.0 * coverage);
        *value = (coverage * alpha * 255.0).round() as u8;
    }
    if let Some(mut pixmap) = PixmapMut::from_bytes(&mut frame.pixels, frame.width, frame.height) {
        pixmap.apply_mask(&mask);
    }
}
