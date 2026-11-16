// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure shapes for the Windows feedback windows, in physical
//! desktop pixels. Kept free of Win32 so the decisions run in any test build.

#![cfg_attr(
    not(windows),
    allow(dead_code, reason = "off Windows only the tests compile this module")
)]

use a7n_proto::v1::Edge;

use crate::input::Bounds;

const PROGRESS_FULL: i32 = 1000;
/// Share of the edge the glow covers before any resistance has built up.
const GLOW_MIN_PERMILLE: i32 = 250;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub(super) fn from_bounds(bounds: Bounds) -> Self {
        let extent = |size: u32| i32::try_from(size).unwrap_or(i32::MAX);
        Self {
            left: bounds.x,
            top: bounds.y,
            right: bounds.x.saturating_add(extent(bounds.width)),
            bottom: bounds.y.saturating_add(extent(bounds.height)),
        }
    }

    #[cfg(windows)]
    pub(super) fn as_rect(self) -> crate::feedback::look::Rect {
        crate::feedback::look::Rect {
            x: f64::from(self.left),
            y: f64::from(self.top),
            width: f64::from(self.width()),
            height: f64::from(self.height()),
        }
    }

    pub(super) fn width(self) -> i32 {
        self.right - self.left
    }

    pub(super) fn height(self) -> i32 {
        self.bottom - self.top
    }

    pub(super) fn intersect(self, other: Self) -> Option<Self> {
        let overlap = Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        };
        (overlap.width() > 0 && overlap.height() > 0).then_some(overlap)
    }
}

/// The glow strip along `edge` of `area`, centred on the pointer. It grows
/// from [`GLOW_MIN_PERMILLE`] of the edge to all of it as `progress` rises.
pub(super) fn glow_rect(
    area: Rect,
    edge: Edge,
    progress: u16,
    thickness: i32,
    pointer: (i32, i32),
) -> Option<Rect> {
    let (span_start, span, along) = match edge {
        Edge::Left | Edge::Right => (area.top, area.height(), pointer.1),
        Edge::Top | Edge::Bottom => (area.left, area.width(), pointer.0),
        Edge::Unspecified => return None,
    };
    let progress = i32::from(progress).min(PROGRESS_FULL);
    let permille =
        GLOW_MIN_PERMILLE + progress * (PROGRESS_FULL - GLOW_MIN_PERMILLE) / PROGRESS_FULL;
    let length = scale(span, permille);
    if length <= 0 || thickness <= 0 {
        return None;
    }
    let start = (along - length / 2).clamp(span_start, span_start + span - length);
    let end = start + length;
    Some(match edge {
        Edge::Left | Edge::Unspecified => Rect {
            left: area.left,
            top: start,
            right: area.left + thickness,
            bottom: end,
        },
        Edge::Right => Rect {
            left: area.right - thickness,
            top: start,
            right: area.right,
            bottom: end,
        },
        Edge::Top => Rect {
            left: start,
            top: area.top,
            right: end,
            bottom: area.top + thickness,
        },
        Edge::Bottom => Rect {
            left: start,
            top: area.bottom - thickness,
            right: end,
            bottom: area.bottom,
        },
    })
}

fn scale(value: i32, permille: i32) -> i32 {
    i32::try_from(i64::from(value) * i64::from(permille) / i64::from(PROGRESS_FULL))
        .unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect {
        left: -1920,
        top: 0,
        right: 0,
        bottom: 1000,
    };

    #[test]
    fn glow_rect_edge_and_progress_hugs_edge_and_grows_inside_area() {
        // Arrange
        let rect = |left, top, right, bottom| Rect {
            left,
            top,
            right,
            bottom,
        };
        let cases = [
            (Edge::Left, 0, (0, 500), rect(-1920, 375, -1914, 625)),
            (Edge::Right, 1000, (0, 500), rect(-6, 0, 0, 1000)),
            (Edge::Top, 0, (-100, 0), rect(-480, 0, 0, 6)),
            (
                Edge::Bottom,
                500,
                (-1900, 999),
                rect(-1920, 994, -720, 1000),
            ),
        ];

        for (edge, progress, pointer, expected) in cases {
            // Act
            let result = glow_rect(AREA, edge, progress, 6, pointer);

            // Assert
            assert_eq!(result, Some(expected), "{edge:?} at {progress}");
        }
    }
}
