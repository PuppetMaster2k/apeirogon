// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_platform::input::Bounds;
use a7n_proto::v1::{Edge, Landing};

#[allow(clippy::cast_possible_truncation)] // Clamp normalized wire offsets to the desktop's i32 coordinates.
pub(super) fn landing_point(bounds: Bounds, landing: &Landing) -> (i32, i32) {
    let edge = Edge::try_from(landing.edge).unwrap_or(Edge::Unspecified);
    let offset = f64::from(landing.offset.clamp(0.0, 1.0));
    let right = f64::from(bounds.x) + f64::from(bounds.width.saturating_sub(1));
    let bottom = f64::from(bounds.y) + f64::from(bounds.height.saturating_sub(1));
    let x = if edge == Edge::Right {
        right
    } else if edge == Edge::Left {
        f64::from(bounds.x)
    } else {
        f64::from(bounds.x) + f64::from(bounds.width.saturating_sub(1)) * offset
    };
    let y = if edge == Edge::Bottom {
        bottom
    } else if edge == Edge::Top {
        f64::from(bounds.y)
    } else {
        f64::from(bounds.y) + f64::from(bounds.height.saturating_sub(1)) * offset
    };
    (
        x.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32,
        y.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32,
    )
}
