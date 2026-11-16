// SPDX-License-Identifier: GPL-3.0-or-later

//! Desktop rectangles for the Windows feedback windows, in physical pixels.

use crate::input::Bounds;

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
