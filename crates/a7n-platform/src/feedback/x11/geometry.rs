// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure placement rules for the edge glow, kept apart from the X calls.

use a7n_proto::v1::Edge;

use super::super::Size;
use crate::input::Bounds;

/// Full resistance, as `progress` per-mille.
const PROGRESS_FULL: u64 = 1000;
/// Thickness of the glow strip in pixels.
const GLOW_THICKNESS: u32 = 6;

/// The monitor containing the pointer, limited to the daemon's `display`
/// bounds. Falls back to `display` when no monitor qualifies.
pub(super) fn pick_monitor(monitors: &[Bounds], display: Bounds, x: i32, y: i32) -> Bounds {
    monitors
        .iter()
        .find(|monitor| contains(**monitor, x, y))
        .and_then(|monitor| intersect(*monitor, display))
        .unwrap_or(display)
}

/// The glow strip along `edge` of `monitor`. Its length grows with `progress`
/// and is centred on the pointer's coordinate `along` the edge. `None` for an
/// unspecified edge.
pub(super) fn glow_rect(
    monitor: Bounds,
    edge: Edge,
    along: i32,
    progress: u16,
    size: Size,
) -> Option<Bounds> {
    let horizontal = match edge {
        Edge::Top | Edge::Bottom => true,
        Edge::Left | Edge::Right => false,
        Edge::Unspecified => return None,
    };
    let at_far_side = matches!(edge, Edge::Bottom | Edge::Right);
    let (start, span, depth_start, depth_span) = if horizontal {
        (monitor.x, monitor.width, monitor.y, monitor.height)
    } else {
        (monitor.y, monitor.height, monitor.x, monitor.width)
    };
    let (from, length) = strip(start, span, along, progress);
    let thickness = size.pixels(GLOW_THICKNESS).min(depth_span);
    let depth = if at_far_side {
        depth_start.saturating_add_unsigned(depth_span - thickness)
    } else {
        depth_start
    };
    Some(if horizontal {
        Bounds {
            x: from,
            y: depth,
            width: length,
            height: thickness,
        }
    } else {
        Bounds {
            x: depth,
            y: from,
            width: thickness,
            height: length,
        }
    })
}

/// Start and length of the strip along an axis of `span` pixels from `start`.
fn strip(start: i32, span: u32, along: i32, progress: u16) -> (i32, u32) {
    let progress = u64::from(progress).min(PROGRESS_FULL);
    let scaled = u64::from(span) * progress / PROGRESS_FULL;
    let length = u32::try_from(scaled).unwrap_or(span).clamp(1, span.max(1));
    let last = start.saturating_add_unsigned(span.saturating_sub(length));
    let from = along.saturating_sub_unsigned(length / 2).clamp(start, last);
    (from, length)
}

fn contains(bounds: Bounds, x: i32, y: i32) -> bool {
    x >= bounds.x
        && y >= bounds.y
        && i64::from(x) < i64::from(bounds.x) + i64::from(bounds.width)
        && i64::from(y) < i64::from(bounds.y) + i64::from(bounds.height)
}

fn intersect(a: Bounds, b: Bounds) -> Option<Bounds> {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (i64::from(a.x) + i64::from(a.width)).min(i64::from(b.x) + i64::from(b.width));
    let bottom = (i64::from(a.y) + i64::from(a.height)).min(i64::from(b.y) + i64::from(b.height));
    let width = u32::try_from(right - i64::from(left))
        .ok()
        .filter(|width| *width > 0)?;
    let height = u32::try_from(bottom - i64::from(top))
        .ok()
        .filter(|height| *height > 0)?;
    Some(Bounds {
        x: left,
        y: top,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn bounds(x: i32, y: i32, width: u32, height: u32) -> Bounds {
        Bounds {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn glow_rect_edge_and_progress_places_and_sizes_strip() {
        // Arrange
        let monitor = bounds(1920, 0, 1000, 800);
        let cases = [
            (Edge::Top, 2420, 500, Some(bounds(2170, 0, 500, 6))),
            (Edge::Bottom, 1930, 500, Some(bounds(1920, 794, 500, 6))),
            (Edge::Left, 100, 1000, Some(bounds(1920, 0, 6, 800))),
            (Edge::Right, 5000, 250, Some(bounds(2914, 600, 6, 200))),
            (Edge::Unspecified, 0, 500, None),
        ];

        for (edge, along, progress, expected) in cases {
            // Act
            let rect = glow_rect(monitor, edge, along, progress, Size::Medium);

            // Assert
            assert_eq!(rect, expected, "{edge:?}");
        }
    }

    #[test]
    fn pick_monitor_pointer_position_selects_containing_monitor() {
        // Arrange
        let monitors = [bounds(0, 0, 1920, 1080), bounds(1920, 0, 1280, 1024)];
        let display = bounds(0, 0, 3200, 1080);
        let cases = [
            (10, 10, bounds(0, 0, 1920, 1080)),
            (2000, 10, bounds(1920, 0, 1280, 1024)),
            (-5, 10, display),
        ];

        for (x, y, expected) in cases {
            // Act
            let monitor = pick_monitor(&monitors, display, x, y);

            // Assert
            assert_eq!(monitor, expected, "({x}, {y})");
        }
    }
}
