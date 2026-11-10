// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::Placement;

// The placements slice may include the candidate's original position.
pub(super) fn overlaps(candidate: &Placement, others: &[Placement]) -> bool {
    let [left, top, right, bottom] = edges(candidate);
    others.iter().filter(|p| p.id != candidate.id).any(|p| {
        let [other_left, other_top, other_right, other_bottom] = edges(p);
        left < other_right && other_left < right && top < other_bottom && other_top < bottom
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GuideKind {
    Flush,
    Alignment,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Guide {
    pub coordinate: i64,
    pub span: [i64; 2],
    pub kind: GuideKind,
}

#[derive(Default)]
pub(super) struct Snapped {
    pub x: Option<Guide>,
    pub y: Option<Guide>,
}

struct AxisSnap {
    delta: i64,
    coordinate: i64,
    other: Placement,
    kind: GuideKind,
}

#[allow(
    clippy::cast_precision_loss,
    reason = "differences of i32 coordinates plus u32 sizes fit exactly in f64"
)]
pub(super) fn snap(
    mut candidate: Placement,
    others: &[Placement],
    threshold: f64,
) -> (Placement, Snapped) {
    let [left, top, right, bottom] = edges(&candidate);
    let mut dx = None;
    let mut dy = None;
    for other in others.iter().filter(|p| p.id != candidate.id) {
        let [other_left, other_top, other_right, other_bottom] = edges(other);
        if (top.max(other_top) - bottom.min(other_bottom)) as f64 <= threshold {
            nearest(
                &mut dx,
                [left, right],
                [other_left, other_right],
                other,
                threshold,
            );
        }
        if (left.max(other_left) - right.min(other_right)) as f64 <= threshold {
            nearest(
                &mut dy,
                [top, bottom],
                [other_top, other_bottom],
                other,
                threshold,
            );
        }
    }
    let x = apply_axis(&mut candidate.x, dx);
    let y = apply_axis(&mut candidate.y, dy);
    let snapped = Snapped {
        x: x.and_then(|s| guide(&candidate, &s, [1, 3])),
        y: y.and_then(|s| guide(&candidate, &s, [0, 2])),
    };
    (candidate, snapped)
}

fn apply_axis(position: &mut i32, target: Option<AxisSnap>) -> Option<AxisSnap> {
    let target = target?;
    *position = i32::try_from(i64::from(*position) + target.delta).ok()?;
    Some(target)
}

fn guide(candidate: &Placement, target: &AxisSnap, [start, end]: [usize; 2]) -> Option<Guide> {
    let from = edges(candidate);
    let to = edges(&target.other);
    let span = match target.kind {
        GuideKind::Flush => [from[start].max(to[start]), from[end].min(to[end])],
        GuideKind::Alignment => [from[start].min(to[start]), from[end].max(to[end])],
    };
    (span[0] < span[1]).then_some(Guide {
        coordinate: target.coordinate,
        span,
        kind: target.kind,
    })
}

#[allow(
    clippy::cast_precision_loss,
    reason = "differences of i32 coordinates plus u32 sizes fit exactly in f64"
)]
fn nearest(
    best: &mut Option<AxisSnap>,
    from: [i64; 2],
    to: [i64; 2],
    other: &Placement,
    threshold: f64,
) {
    for (i, edge) in from.into_iter().enumerate() {
        for (j, target) in to.into_iter().enumerate() {
            let delta = target - edge;
            if delta.abs() as f64 <= threshold
                && best.as_ref().is_none_or(|b| delta.abs() < b.delta.abs())
            {
                *best = Some(AxisSnap {
                    delta,
                    coordinate: target,
                    other: *other,
                    kind: if i == j {
                        GuideKind::Alignment
                    } else {
                        GuideKind::Flush
                    },
                });
            }
        }
    }
}

fn edges(p: &Placement) -> [i64; 4] {
    [
        i64::from(p.x),
        i64::from(p.y),
        i64::from(p.x) + i64::from(p.width),
        i64::from(p.y) + i64::from(p.height),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement(id: u8, x: i32, y: i32) -> Placement {
        Placement {
            id: a7n_ipc::MachineId::try_from([id; 16].as_slice()).unwrap(),
            x,
            y,
            width: 100,
            height: 100,
            blocked: 0,
        }
    }

    #[test]
    fn overlaps_touching_and_intersecting_edges_distinguishes_interiors() {
        // Arrange
        let others = [placement(1, 0, 0)];
        let cases = [
            (100, 0, false),
            (99, 0, true),
            (0, 100, false),
            (0, 99, true),
        ];

        for (x, y, expected) in cases {
            // Act
            let result = overlaps(&placement(2, x, y), &others);

            // Assert
            assert_eq!(result, expected);
        }
    }
}
