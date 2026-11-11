// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_proto::v1::{Edge, Landing};

use super::clamp_position;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Bounds {
    #[must_use]
    pub const fn rect(self) -> (i32, i32, u32, u32) {
        (self.x, self.y, self.width, self.height)
    }

    pub(super) fn contains(self, x: i64, y: i64) -> bool {
        x >= i64::from(self.x)
            && y >= i64::from(self.y)
            && x < i64::from(self.x) + i64::from(self.width)
            && y < i64::from(self.y) + i64::from(self.height)
    }
}

impl From<(i32, i32, u32, u32)> for Bounds {
    fn from((x, y, width, height): (i32, i32, u32, u32)) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

pub(super) fn nearest_monitor(
    bounds: (i32, i32, u32, u32),
    monitors: &[Bounds],
    x: i32,
    y: i32,
) -> Bounds {
    monitors
        .iter()
        .filter(|m| m.width > 0 && m.height > 0)
        .min_by_key(|m| {
            let (px, py) = clamp_position(m.rect(), x, y);
            let dx = i128::from(x) - i128::from(px);
            let dy = i128::from(y) - i128::from(py);
            dx * dx + dy * dy
        })
        .copied()
        .unwrap_or_else(|| bounds.into())
}

/// Project a bbox-relative landing onto the outer monitor edge in that direction.
#[must_use]
#[allow(clippy::cast_possible_truncation)] // Offsets are clamped and float-to-int casts saturate.
pub fn landing_point(bounds: Bounds, monitors: &[Bounds], landing: &Landing) -> (i32, i32) {
    let edge = Edge::try_from(landing.edge).unwrap_or(Edge::Unspecified);
    let offset = f64::from(landing.offset.clamp(0.0, 1.0));
    let right = bounds
        .x
        .saturating_add_unsigned(bounds.width.saturating_sub(1));
    let bottom = bounds
        .y
        .saturating_add_unsigned(bounds.height.saturating_sub(1));
    let x = match edge {
        Edge::Left => bounds.x,
        Edge::Right => right,
        _ => (f64::from(bounds.x) + f64::from(bounds.width.saturating_sub(1)) * offset) as i32,
    };
    let y = match edge {
        Edge::Top => bounds.y,
        Edge::Bottom => bottom,
        _ => (f64::from(bounds.y) + f64::from(bounds.height.saturating_sub(1)) * offset) as i32,
    };
    let horizontal = matches!(edge, Edge::Left | Edge::Right);
    let monitor = monitors
        .iter()
        .filter(|m| m.width > 0 && m.height > 0)
        .min_by_key(|m| {
            let (px, py) = clamp_position(m.rect(), x, y);
            let distance = if horizontal {
                i64::from(y) - i64::from(py)
            } else {
                i64::from(x) - i64::from(px)
            };
            let boundary = match edge {
                Edge::Left => i64::from(m.x),
                Edge::Right => -(i64::from(m.x) + i64::from(m.width) - 1),
                Edge::Top => i64::from(m.y),
                Edge::Bottom => -(i64::from(m.y) + i64::from(m.height) - 1),
                Edge::Unspecified => 0,
            };
            (distance.abs(), boundary)
        })
        .copied()
        .unwrap_or(bounds);
    let (px, py) = clamp_position(monitor.rect(), x, y);
    match edge {
        Edge::Left => (monitor.x, py),
        Edge::Right => (
            monitor
                .x
                .saturating_add_unsigned(monitor.width.saturating_sub(1)),
            py,
        ),
        Edge::Top => (px, monitor.y),
        Edge::Bottom => (
            px,
            monitor
                .y
                .saturating_add_unsigned(monitor.height.saturating_sub(1)),
        ),
        Edge::Unspecified => (x, y),
    }
}
