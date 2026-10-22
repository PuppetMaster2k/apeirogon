// SPDX-License-Identifier: GPL-3.0-or-later

//! Allocation-free cumulative pointer motion and desktop edge detection.

use a7n_proto::seq::{delta, is_newer};
use a7n_proto::v1::{Edge, Motion};

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

/// Detect outward motion on an edge pixel; horizontal motion wins at corners.
#[must_use]
#[allow(clippy::cast_possible_truncation)] // Normalized fractions in 0..=1 fit f32.
pub fn edge_push(
    (left, top, width, height): (i32, i32, u32, u32),
    x: i32,
    y: i32,
    dx: i32,
    dy: i32,
) -> Option<(Edge, i32, f32)> {
    if width == 0 || height == 0 {
        return None;
    }
    let right = i64::from(left) + i64::from(width) - 1;
    let bottom = i64::from(top) + i64::from(height) - 1;
    if i64::from(x) < i64::from(left)
        || i64::from(x) > right
        || i64::from(y) < i64::from(top)
        || i64::from(y) > bottom
    {
        return None;
    }
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

#[cfg(test)]
mod tests;
