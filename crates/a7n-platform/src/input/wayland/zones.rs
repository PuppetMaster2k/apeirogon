// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure geometry of the compositor's zones: bounds, containment and the
//! pointer barriers along the outer edge of the zone union.

use crate::input::Bounds;

/// A pointer barrier as the `InputCapture` portal wants it: a horizontal or
/// vertical line whose end points are inclusive pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Barrier {
    pub(super) x1: i32,
    pub(super) y1: i32,
    pub(super) x2: i32,
    pub(super) y2: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

const SIDES: [Side; 4] = [Side::Left, Side::Right, Side::Top, Side::Bottom];

/// Half-open interval `[start, end)` along an edge, wide enough that edge
/// arithmetic cannot overflow.
type Span = (i64, i64);

/// Smallest rectangle holding every zone, or `None` without zones.
pub(super) fn bounding_box(zones: &[Bounds]) -> Option<Bounds> {
    let left = zones.iter().map(|zone| i64::from(zone.x)).min()?;
    let top = zones.iter().map(|zone| i64::from(zone.y)).min()?;
    let right = zones.iter().map(|zone| right_edge(*zone)).max()?;
    let bottom = zones.iter().map(|zone| bottom_edge(*zone)).max()?;
    Some(Bounds {
        x: i32::try_from(left).ok()?,
        y: i32::try_from(top).ok()?,
        width: u32::try_from(right - left).ok()?,
        height: u32::try_from(bottom - top).ok()?,
    })
}

pub(super) fn contains(zones: &[Bounds], x: i32, y: i32) -> bool {
    zones.iter().any(|zone| zone_contains(*zone, x, y))
}

/// The point of the zone union closest to `(x, y)`, which is `(x, y)` itself
/// when it is already inside.
pub(super) fn nearest_inside(zones: &[Bounds], x: i32, y: i32) -> Option<(i32, i32)> {
    zones
        .iter()
        .map(|zone| clamp_into(*zone, x, y))
        .min_by_key(|&(nx, ny)| {
            let (dx, dy) = (i64::from(nx) - i64::from(x), i64::from(ny) - i64::from(y));
            dx * dx + dy * dy
        })
}

/// Barriers along the boundary of the zone union. An edge stretch that another
/// zone covers on its far side is interior, so side-by-side monitors get no
/// barrier where they touch. Each barrier lies within a single zone, as the
/// portal requires.
pub(super) fn outer_barriers(zones: &[Bounds]) -> Vec<Barrier> {
    let mut barriers = Vec::new();
    for (index, zone) in zones.iter().enumerate() {
        for side in SIDES {
            let covered: Vec<Span> = zones
                .iter()
                .enumerate()
                .filter(|&(other, _)| other != index)
                .filter_map(|(_, other)| covered_span(*zone, side, *other))
                .collect();
            for (start, end) in uncovered(edge_span(*zone, side), covered) {
                barriers.push(barrier(*zone, side, start, end));
            }
        }
    }
    barriers
}

fn right_edge(zone: Bounds) -> i64 {
    i64::from(zone.x) + i64::from(zone.width)
}

fn bottom_edge(zone: Bounds) -> i64 {
    i64::from(zone.y) + i64::from(zone.height)
}

fn zone_contains(zone: Bounds, x: i32, y: i32) -> bool {
    let (x, y) = (i64::from(x), i64::from(y));
    x >= i64::from(zone.x)
        && x < right_edge(zone)
        && y >= i64::from(zone.y)
        && y < bottom_edge(zone)
}

fn clamp_into(zone: Bounds, x: i32, y: i32) -> (i32, i32) {
    let clamp = |value: i32, origin: i32, length: u32| {
        let last = i64::from(origin) + i64::from(length) - 1;
        let clamped = i64::from(value).clamp(i64::from(origin), last.max(i64::from(origin)));
        i32::try_from(clamped).expect("clamped between two i32 values")
    };
    (clamp(x, zone.x, zone.width), clamp(y, zone.y, zone.height))
}

/// The stretch of `zone`'s edge on `side`, along the axis the edge runs on.
fn edge_span(zone: Bounds, side: Side) -> Span {
    match side {
        Side::Left | Side::Right => (i64::from(zone.y), bottom_edge(zone)),
        Side::Top | Side::Bottom => (i64::from(zone.x), right_edge(zone)),
    }
}

/// The part of `zone`'s edge on `side` that `other` covers just beyond it.
fn covered_span(zone: Bounds, side: Side, other: Bounds) -> Option<Span> {
    let outside = match side {
        Side::Left => i64::from(zone.x) - 1,
        Side::Right => right_edge(zone),
        Side::Top => i64::from(zone.y) - 1,
        Side::Bottom => bottom_edge(zone),
    };
    let (own_start, own_end) = edge_span(zone, side);
    let (across_start, across_end, along_start, along_end) = match side {
        Side::Left | Side::Right => (
            i64::from(other.x),
            right_edge(other),
            i64::from(other.y),
            bottom_edge(other),
        ),
        Side::Top | Side::Bottom => (
            i64::from(other.y),
            bottom_edge(other),
            i64::from(other.x),
            right_edge(other),
        ),
    };
    if outside < across_start || outside >= across_end {
        return None;
    }
    let (start, end) = (along_start.max(own_start), along_end.min(own_end));
    (start < end).then_some((start, end))
}

/// `span` minus every covered stretch, in order.
fn uncovered(span: Span, mut covered: Vec<Span>) -> Vec<Span> {
    covered.sort_unstable();
    let mut parts = Vec::new();
    let mut cursor = span.0;
    for (start, end) in covered {
        if start > cursor {
            parts.push((cursor, start));
        }
        cursor = cursor.max(end);
    }
    if cursor < span.1 {
        parts.push((cursor, span.1));
    }
    parts
}

/// Portal barriers sit on the top or left edge of their pixels, so the far
/// edges of a zone are one pixel beyond its last pixel.
fn barrier(zone: Bounds, side: Side, start: i64, end: i64) -> Barrier {
    let to_i32 = |value: i64| i32::try_from(value).expect("derived from i32 zone coordinates");
    let (first, last) = (to_i32(start), to_i32(end - 1));
    match side {
        Side::Left => Barrier {
            x1: zone.x,
            y1: first,
            x2: zone.x,
            y2: last,
        },
        Side::Right => {
            let x = to_i32(right_edge(zone));
            Barrier {
                x1: x,
                y1: first,
                x2: x,
                y2: last,
            }
        }
        Side::Top => Barrier {
            x1: first,
            y1: zone.y,
            x2: last,
            y2: zone.y,
        },
        Side::Bottom => {
            let y = to_i32(bottom_edge(zone));
            Barrier {
                x1: first,
                y1: y,
                x2: last,
                y2: y,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(x: i32, y: i32, width: u32, height: u32) -> Bounds {
        Bounds {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn outer_barriers_side_by_side_monitors_have_no_barrier_on_shared_edge() {
        // Arrange
        let zones = [zone(0, 0, 1920, 1080), zone(1920, 0, 1920, 1080)];

        // Act
        let barriers = outer_barriers(&zones);

        // Assert
        let on_seam = |barrier: &Barrier| barrier.x1 == barrier.x2 && barrier.x1 == 1920;
        assert!(!barriers.iter().any(on_seam));
        assert_eq!(barriers.len(), 6);
        assert!(barriers.contains(&Barrier {
            x1: 0,
            y1: 0,
            x2: 0,
            y2: 1079
        }));
        assert!(barriers.contains(&Barrier {
            x1: 3840,
            y1: 0,
            x2: 3840,
            y2: 1079
        }));
        assert!(barriers.contains(&Barrier {
            x1: 1920,
            y1: 1080,
            x2: 3839,
            y2: 1080
        }));
    }

    #[test]
    fn outer_barriers_shorter_neighbour_leaves_the_exposed_part_of_the_seam() {
        // Arrange
        let zones = [zone(0, 0, 1000, 1000), zone(1000, 0, 1000, 400)];

        // Act
        let barriers = outer_barriers(&zones);

        // Assert
        let seam: Vec<_> = barriers
            .iter()
            .filter(|b| b.x1 == 1000 && b.x2 == 1000)
            .collect();
        assert_eq!(
            seam,
            [&Barrier {
                x1: 1000,
                y1: 400,
                x2: 1000,
                y2: 999
            }]
        );
    }

    #[test]
    fn bounding_box_two_monitors_spans_both() {
        // Arrange
        let zones = [zone(0, 0, 1920, 1080), zone(1920, -200, 1280, 1024)];

        // Act
        let bounds = bounding_box(&zones);

        // Assert
        assert_eq!(bounds, Some(zone(0, -200, 3200, 1280)));
    }

    #[test]
    fn nearest_inside_point_beyond_edge_lands_on_last_pixel() {
        // Arrange
        let zones = [zone(0, 0, 1920, 1080)];

        // Act
        let inside = nearest_inside(&zones, 1950, 500);

        // Assert
        assert_eq!(inside, Some((1919, 500)));
    }
}
