// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared premultiplied RGBA frames; native backends only place and blit them.

use std::time::Duration;

use a7n_proto::v1::Edge;

use tiny_skia::{
    FillRule, GradientStop, LineCap, LinearGradient, Paint, Path, PathBuilder, PixmapMut, Point,
    RadialGradient, Rect, Shader, SpreadMode, Stroke, SweepGradient, Transform,
};

use super::timeline::Motion;
use super::{Landing, Look, Style};

const CHANNELS: usize = 4;
const MEMBRANE_COLOR_SHIFT: f32 = 0.35;
const MEMBRANE_TIP_TINT: f32 = 0.45;
const MEMBRANE_DARK_TIP_TINT: f32 = 0.55;

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

pub(super) const MEMBRANE_DEPTH: u32 = 96;

pub(super) const fn edge_depth(style: Style) -> u32 {
    if matches!(style, Style::Membrane) {
        MEMBRANE_DEPTH
    } else {
        48
    }
}
pub(super) const MEMBRANE_ARRIVAL_SIZE: u32 = 224;
const ARRIVAL_SIZE: u32 = 160;

pub(super) const fn arrival_size(style: Style) -> u32 {
    if matches!(style, Style::Membrane) {
        MEMBRANE_ARRIVAL_SIZE
    } else {
        ARRIVAL_SIZE
    }
}
const PROGRESS_FULL: f32 = 1000.0;

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
            .filter(|(distance, _)| *distance < rect.width.min(rect.height) / 2.0)
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

// Native desktop spans and elapsed animation values fit skia's f32 geometry.
#[allow(clippy::cast_possible_truncation)]
pub(super) fn render_departure(frame: &mut Frame, effect: EdgeEffect, mut motion: Motion) {
    let animate = frame.animate;
    if !animate {
        motion.seconds = 0.4;
        motion.tick = 0.0;
        motion.finish = None;
    }
    let Some(mut canvas) = Canvas::new(frame, effect.look) else {
        return;
    };
    let (length, depth) = canvas.orient(effect.edge);
    let start = -effect.along.start as f32;
    let end = start + effect.along.span as f32;
    canvas.linear(start, end, 0.0, 1.0);
    if effect.look.style.animated() {
        super::effects::departure(
            &mut canvas,
            &super::effects::Push {
                style: effect.look.style,
                length,
                depth,
                pointer: effect.along.push as f32,
                progress: (f32::from(effect.progress) / PROGRESS_FULL).min(1.0),
                motion,
            },
        );
    } else if let Some(rect) = Rect::from_xywh(0.0, 0.0, length, depth) {
        canvas.fill(&PathBuilder::from_rect(rect));
    }
}

// The shared frame is bounded by the native overlay size.
#[allow(clippy::cast_possible_truncation)]
pub(super) fn render_landing(frame: &mut Frame, look: Look, elapsed: Duration, duration: Duration) {
    let (radius, alpha) = look.landing.frame(elapsed, duration, frame.animate);
    let stroke = frame.stroke as f32;
    let animate = frame.animate;
    let arrival = frame.arrival;
    let phase = (elapsed.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0);
    let alpha = if animate { alpha as f32 } else { 1.0 - phase };
    let Some(mut canvas) = Canvas::new(frame, look) else {
        return;
    };
    let center = Point::from_xy(canvas.width / 2.0, canvas.height / 2.0);
    if look.style.animated() {
        super::effects::arrival(
            &mut canvas,
            look.style,
            arrival.unwrap_or(Arrival {
                center: (center.x, center.y),
                edge: None,
            }),
            phase,
            animate,
        );
        return;
    }
    let radius = ((canvas.width.min(canvas.height) - stroke) / 2.0 * radius as f32).max(0.01);
    let stops = canvas.stops(alpha);
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
    .unwrap_or_else(|| canvas.solid(0.0, alpha));
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
    pub width: f32,
    pub height: f32,
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
            width: frame.width as f32,
            height: frame.height as f32,
            look,
            dark: frame.dark,
            opacity: frame.opacity as f32,
        })
    }

    pub fn orient(&mut self, edge: Edge) -> (f32, f32) {
        self.transform = match edge {
            Edge::Top => Transform::identity(),
            Edge::Bottom => Transform::from_row(1.0, 0.0, 0.0, -1.0, 0.0, self.height),
            Edge::Left | Edge::Unspecified => Transform::from_row(0.0, 1.0, 1.0, 0.0, 0.0, 0.0),
            Edge::Right => Transform::from_row(0.0, 1.0, -1.0, 0.0, self.width, 0.0),
        };
        if matches!(edge, Edge::Top | Edge::Bottom) {
            (self.width, self.height)
        } else {
            (self.height, self.width)
        }
    }

    pub fn linear(&mut self, start: f32, end: f32, shift: f32, alpha: f32) {
        let stops = (0..=48_u16)
            .map(|i| {
                let t = f32::from(i) / 48.0;
                GradientStop::new(t, self.color(t + shift, alpha))
            })
            .collect();
        self.paint.shader = LinearGradient::new(
            Point::from_xy(start, 0.0),
            Point::from_xy(end, 0.0),
            stops,
            SpreadMode::Pad,
            Transform::identity(),
        )
        .unwrap_or_else(|| self.solid(shift, alpha));
    }

    pub fn soft(&mut self, center: (f32, f32), radius: f32, position: f32, alpha: f32) {
        let center = Point::from_xy(center.0, center.1);
        let stops = [0.0, 0.25, 1.0]
            .into_iter()
            .map(|t| {
                GradientStop::new(t, self.color(position + t * 0.3, alpha * (1.0 - t).powi(2)))
            })
            .collect();
        self.paint.shader = RadialGradient::new(
            center,
            0.0,
            center,
            radius.max(0.01),
            stops,
            SpreadMode::Pad,
            Transform::identity(),
        )
        .unwrap_or_else(|| self.solid(position, alpha));
    }

    pub fn ring(&mut self, center: (f32, f32), radius: f32, width: f32, alpha: f32) {
        self.paint.shader = SweepGradient::new(
            Point::from_xy(center.0, center.1),
            0.0,
            360.0,
            self.stops(alpha),
            SpreadMode::Pad,
            Transform::identity(),
        )
        .unwrap_or_else(|| self.solid(0.0, alpha));
        self.circle(center, radius, Some(width));
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

    pub fn membrane_color(&mut self, position: f32, pressure: f32, alpha: f32) {
        let color = self.color(position + pressure * MEMBRANE_COLOR_SHIFT, alpha);
        let tint = pressure
            * if self.dark {
                MEMBRANE_DARK_TIP_TINT
            } else {
                MEMBRANE_TIP_TINT
            };
        let color = tiny_skia::Color::from_rgba(
            color.red() + (1.0 - color.red()) * tint,
            color.green() + (1.0 - color.green()) * tint,
            color.blue() + (1.0 - color.blue()) * tint,
            color.alpha(),
        )
        .unwrap_or(color);
        self.paint.shader = Shader::SolidColor(color);
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
