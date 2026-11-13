// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared premultiplied RGBA frames; native backends only place and blit them.

use std::time::Duration;

use a7n_proto::v1::Edge;

use tiny_skia::{
    FillRule, GradientStop, LineCap, Paint, Path, PathBuilder, PixmapMut, Point, RadialGradient,
    Shader, SpreadMode, Stroke, SweepGradient, Transform,
};

use super::{Landing, Look};

const CHANNELS: usize = 4;
const LANDING_EDGE_INSET: f64 = 32.0;
#[derive(Default)]
pub(super) struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub animate: bool,
    pub dark: bool,
    pub opacity: f64,
    pub stroke: f64,
    pub arrival: Option<Arrival>,
    pub(super) studio: super::effects::State,
    pub scale: f64,
    pub marker_diameter: f64,
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
pub(super) const ARRIVAL_DIAMETER: u32 = 1080;

#[derive(Clone, Copy)]
pub(super) struct Arrival {
    pub center: (f32, f32),
    pub edge: Option<Edge>,
}

impl Arrival {
    // Native geometry is bounded by the monitor before conversion to raster coordinates.
    #[allow(clippy::cast_possible_truncation)]
    pub fn at(
        area: super::look::Rect,
        rect: super::look::Rect,
        point: (f64, f64),
        scale: f64,
    ) -> Self {
        let distances = [
            (point.0 - area.x, Edge::Left),
            (area.x + area.width - point.0, Edge::Right),
            (point.1 - area.y, Edge::Top),
            (area.y + area.height - point.1, Edge::Bottom),
        ];
        let edge = distances
            .into_iter()
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .filter(|(distance, _)| *distance <= LANDING_EDGE_INSET)
            .map(|(_, edge)| edge);
        Self {
            center: (
                ((point.0 - rect.x) * scale) as f32,
                ((point.1 - rect.y) * scale) as f32,
            ),
            edge,
        }
    }
}

pub(super) fn render_departure(frame: &mut Frame, effect: EdgeEffect, seconds: f32) {
    super::effects::departure(frame, effect, seconds);
}

pub(super) fn render_landing(frame: &mut Frame, look: Look, elapsed: Duration, duration: Duration) {
    super::effects::arrival(frame, look, elapsed, duration);
}

// Marker geometry and palette are unchanged; its radius no longer depends on the effect window.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub(super) fn render_marker(frame: &mut Frame, look: Look, elapsed: Duration, duration: Duration) {
    let (radius, alpha) = look.landing.frame(elapsed, duration, frame.animate);
    let stroke = frame.stroke as f32;
    let center = frame
        .arrival
        .map_or((frame.width as f32 / 2.0, frame.height as f32 / 2.0), |a| {
            a.center
        });
    let diameter = frame.marker_diameter as f32;
    let alpha = if frame.animate {
        alpha
    } else {
        1.0 - (elapsed.as_secs_f64() / duration.as_secs_f64()).clamp(0.0, 1.0)
    };
    let Some(mut canvas) = Canvas::new(frame, look) else {
        return;
    };
    let center = Point::from_xy(center.0, center.1);
    let radius = ((diameter - stroke) / 2.0 * radius as f32).max(0.01);
    let stops = canvas.stops(alpha as f32);
    canvas.paint.shader = if look.landing == Landing::Dot {
        RadialGradient::new(
            center,
            0.0,
            center,
            radius,
            stops,
            SpreadMode::Pad,
            Transform::identity(),
        )
    } else {
        SweepGradient::new(
            center,
            0.0,
            360.0,
            stops,
            SpreadMode::Pad,
            Transform::from_rotate_at(180.0, center.x, center.y),
        )
    }
    .unwrap_or_else(|| canvas.solid(0.0, alpha as f32));
    canvas.circle(
        (center.x, center.y),
        radius,
        (look.landing != Landing::Dot).then_some(stroke),
    );
}

pub(super) struct Canvas<'a> {
    pixmap: PixmapMut<'a>,
    pub paint: Paint<'static>,
    pub transform: Transform,
    look: Look,
    dark: bool,
    opacity: f32,
}

impl<'a> Canvas<'a> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    pub fn new(frame: &'a mut Frame, look: Look) -> Option<Self> {
        frame.pixels.fill(0);
        Some(Self {
            pixmap: PixmapMut::from_bytes(&mut frame.pixels, frame.width, frame.height)?,
            paint: Paint::default(),
            transform: Transform::identity(),
            look,
            dark: frame.dark,
            opacity: frame.opacity as f32,
        })
    }

    pub fn solid(&self, position: f32, alpha: f32) -> Shader<'static> {
        Shader::SolidColor(self.color(position, alpha))
    }

    fn color(&self, position: f32, alpha: f32) -> tiny_skia::Color {
        let [r, g, b] = self.look.color.sample(position, self.dark);
        tiny_skia::Color::from_rgba(
            f32::from(r) / 255.0,
            f32::from(g) / 255.0,
            f32::from(b) / 255.0,
            (alpha * self.opacity).clamp(0.0, 1.0),
        )
        .unwrap_or(tiny_skia::Color::TRANSPARENT)
    }

    pub fn stops(&self, alpha: f32) -> Vec<GradientStop> {
        const SAMPLES: u16 = 48;
        (0..=SAMPLES)
            .map(|i| {
                let t = f32::from(i) / f32::from(SAMPLES);
                GradientStop::new(t, self.color(t, alpha))
            })
            .collect()
    }

    pub fn fill(&mut self, path: &Path) {
        self.pixmap
            .fill_path(path, &self.paint, FillRule::Winding, self.transform, None);
    }

    pub fn stroke(&mut self, path: &Path, width: f32) {
        self.pixmap.stroke_path(
            path,
            &self.paint,
            &Stroke {
                width,
                line_cap: LineCap::Round,
                ..Stroke::default()
            },
            self.transform,
            None,
        );
    }

    pub fn circle(&mut self, center: (f32, f32), radius: f32, stroke: Option<f32>) {
        if let Some(path) = PathBuilder::from_circle(center.0, center.1, radius) {
            if let Some(width) = stroke {
                self.stroke(&path, width);
            } else {
                self.fill(&path);
            }
        }
    }
}
