// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared premultiplied RGBA frames; native backends only place and blit them.

use std::time::Duration;

use a7n_proto::v1::Edge;

use tiny_skia::{Mask, PixmapMut};

use super::{Landing, Look, look};

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
    #[cfg(any(test, windows, all(unix, not(target_os = "macos"))))]
    pub(super) fn copy_bgra_into(&self, dst: &mut [u8]) {
        debug_assert_eq!(dst.len(), self.pixels.len());
        for (src, dst) in self
            .pixels
            .chunks_exact(CHANNELS)
            .zip(dst.chunks_exact_mut(CHANNELS))
        {
            dst.copy_from_slice(&[src[2], src[1], src[0], src[3]]);
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if (self.width, self.height) != (width, height) {
            self.width = width;
            self.height = height;
            self.pixels
                .resize(width as usize * height as usize * CHANNELS, 0);
        }
        self.pixels.fill(0);
    }

    pub(super) fn prepare(&mut self, width: u32, height: u32, scale: f64, animate: bool) {
        self.resize(width, height);
        self.dark = true;
        self.opacity = 1.0;
        self.scale = scale;
        self.animate = animate;
    }

    /// Whether the frame is non-empty and its pixels match its size, which
    /// the native copies rely on before writing into OS-owned buffers.
    pub fn is_drawable(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.pixels.len() == self.width as usize * self.height as usize * CHANNELS
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

impl Along {
    pub(super) fn across(
        area: look::Rect,
        strip: look::Rect,
        pointer: (f64, f64),
        edge: Edge,
        scale: f64,
    ) -> Self {
        let horizontal = matches!(edge, Edge::Top | Edge::Bottom);
        Self {
            origin: (area.x, area.y),
            start: if horizontal {
                (strip.x - area.x) * scale
            } else {
                (strip.y - area.y) * scale
            },
            span: if horizontal { area.width } else { area.height } * scale,
            push: if horizontal {
                (pointer.0 - strip.x) * scale
            } else {
                (pointer.1 - strip.y) * scale
            },
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_bgra_into_rgba_frame_writes_bgra() {
        // Arrange
        let frame = Frame {
            width: 2,
            height: 1,
            pixels: vec![10, 20, 30, 40, 50, 60, 70, 80],
            ..Frame::default()
        };
        let mut dst = [0; 8];

        // Act
        frame.copy_bgra_into(&mut dst);

        // Assert
        assert_eq!(dst, [30, 20, 10, 40, 70, 60, 50, 80]);
    }

    #[test]
    fn across_all_edges_outside_pointer_preserves_scaled_positions() {
        // Arrange
        let area = look::Rect {
            x: -100.0,
            y: 50.0,
            width: 400.0,
            height: 300.0,
        };
        let strip = look::Rect {
            x: -80.0,
            y: 80.0,
            width: 200.0,
            height: 100.0,
        };
        let pointer = (-120.0, 400.0);
        let scale = 2.0;
        let cases = [
            (Edge::Top, (40.0, 800.0, -80.0)),
            (Edge::Bottom, (40.0, 800.0, -80.0)),
            (Edge::Left, (60.0, 600.0, 640.0)),
            (Edge::Right, (60.0, 600.0, 640.0)),
        ];

        for (edge, expected) in cases {
            // Act
            let along = Along::across(area, strip, pointer, edge, scale);

            // Assert
            assert_eq!(along.origin, (area.x, area.y), "{edge:?}");
            assert_eq!((along.start, along.span, along.push), expected, "{edge:?}");
        }
    }
}
