// SPDX-License-Identifier: GPL-3.0-or-later

//! Allocation-free cumulative pointer motion and desktop edge detection.

use a7n_proto::seq::{delta, is_newer};
use a7n_proto::v1::{Edge, Motion};

use crate::layout::Corner;

/// Maximum distance in pixels from an edge end for a diagonal crossing.
pub const CORNER_BOX: i64 = 8;

/// Receiver-side speed scaling, retaining subpixel motion between updates.
#[derive(Debug)]
pub struct MotionScale {
    speed: f64,
    remainder: (f64, f64),
}

impl MotionScale {
    /// `speed` has already been validated at the configuration boundary.
    #[must_use]
    pub fn new(speed: f64) -> Self {
        Self {
            speed,
            remainder: (0.0, 0.0),
        }
    }

    pub fn apply(&mut self, dx: i32, dy: i32) -> (i32, i32) {
        (
            Self::axis(dx, self.speed, &mut self.remainder.0),
            Self::axis(dy, self.speed, &mut self.remainder.1),
        )
    }

    #[allow(clippy::cast_possible_truncation)] // Pixel deltas truncate toward zero and saturate at i32 bounds.
    fn axis(delta: i32, speed: f64, remainder: &mut f64) -> i32 {
        let scaled = f64::from(delta) * speed + *remainder;
        *remainder = scaled.fract();
        scaled as i32
    }
}

#[derive(Debug, Default)]
pub struct MotionCounter {
    seq: u32,
    x: i32,
    y: i32,
}

impl MotionCounter {
    pub fn push(&mut self, dx: i32, dy: i32) -> Motion {
        self.seq = self.seq.wrapping_add(1);
        self.x = self.x.wrapping_add(dx);
        self.y = self.y.wrapping_add(dy);
        self.current()
    }

    /// The daemon supplies the route for this snapshot.
    #[must_use]
    pub fn current(&self) -> Motion {
        Motion {
            route: 0,
            motion_seq: self.seq,
            x: self.x,
            y: self.y,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Debug, Default)]
pub struct MotionTracker {
    seq: u32,
    x: i32,
    y: i32,
}

impl MotionTracker {
    pub fn apply(&mut self, seq: u32, x: i32, y: i32) -> Option<(i32, i32)> {
        if !is_newer(seq, self.seq) {
            return None;
        }
        Some(self.update(seq, x, y))
    }

    pub fn resync(&mut self, seq: u32, x: i32, y: i32) -> Option<(i32, i32)> {
        if is_newer(seq, self.seq) || (seq == self.seq && (x, y) != (self.x, self.y)) {
            return Some(self.update(seq, x, y));
        }
        None
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    fn update(&mut self, seq: u32, x: i32, y: i32) -> (i32, i32) {
        let movement = (delta(x, self.x), delta(y, self.y));
        (self.seq, self.x, self.y) = (seq, x, y);
        movement
    }
}

/// Clamp a desktop point to its bounds, including negative origins.
#[must_use]
pub fn clamp_position(
    (left, top, width, height): (i32, i32, u32, u32),
    x: i32,
    y: i32,
) -> (i32, i32) {
    (
        x.clamp(left, left.saturating_add_unsigned(width.saturating_sub(1))),
        y.clamp(top, top.saturating_add_unsigned(height.saturating_sub(1))),
    )
}

/// Detect outward motion at or beyond an edge; horizontal motion wins at corners.
#[must_use]
#[allow(clippy::cast_possible_truncation)] // Normalized fractions in 0..=1 fit f32.
pub fn edge_push(
    bounds @ (left, top, width, height): (i32, i32, u32, u32),
    x: i32,
    y: i32,
    dx: i32,
    dy: i32,
) -> Option<(Edge, i32, f32)> {
    if width == 0 || height == 0 {
        return None;
    }
    let (x, y) = clamp_position(bounds, x, y);
    let right = i64::from(left) + i64::from(width) - 1;
    let bottom = i64::from(top) + i64::from(height) - 1;
    let (edge, overshoot, position, start, length) = if x == left && dx < 0 {
        (Edge::Left, dx.saturating_neg(), y, top, height)
    } else if i64::from(x) == right && dx > 0 {
        (Edge::Right, dx, y, top, height)
    } else if y == top && dy < 0 {
        (Edge::Top, dy.saturating_neg(), x, left, width)
    } else if i64::from(y) == bottom && dy > 0 {
        (Edge::Bottom, dy, x, left, width)
    } else {
        return None;
    };
    let along =
        (f64::from(position) - f64::from(start)) / f64::from(length.saturating_sub(1).max(1));
    Some((edge, overshoot, along as f32))
}

/// Detect an edge push whose other axis points toward a nearby corner.
#[must_use]
pub fn corner_push(
    bounds @ (left, top, width, height): (i32, i32, u32, u32),
    x: i32,
    y: i32,
    dx: i32,
    dy: i32,
) -> Option<(Edge, Corner)> {
    let (edge, _, _) = edge_push(bounds, x, y, dx, dy)?;
    let (x, y) = clamp_position(bounds, x, y);
    let right = i64::from(left) + i64::from(width) - 1;
    let bottom = i64::from(top) + i64::from(height) - 1;
    let (position, start, end, delta) = match edge {
        Edge::Left | Edge::Right => (y, top, bottom, dy),
        Edge::Top | Edge::Bottom => (x, left, right, dx),
        Edge::Unspecified => return None,
    };
    let at_end = if delta < 0 && i64::from(position) - i64::from(start) <= CORNER_BOX {
        false
    } else if delta > 0 && end - i64::from(position) <= CORNER_BOX {
        true
    } else {
        return None;
    };
    let corner = match (edge, at_end) {
        (Edge::Left | Edge::Top, false) => Corner::TopLeft,
        (Edge::Right, false) | (Edge::Top, true) => Corner::TopRight,
        (Edge::Left, true) | (Edge::Bottom, false) => Corner::BottomLeft,
        (Edge::Right | Edge::Bottom, true) => Corner::BottomRight,
        (Edge::Unspecified, _) => return None,
    };
    Some((edge, corner))
}

#[cfg(test)]
mod tests;
