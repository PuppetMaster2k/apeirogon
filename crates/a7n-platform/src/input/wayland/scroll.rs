// SPDX-License-Identifier: GPL-3.0-or-later

//! A peer's scroll event as a libei scroll request.

use a7n_proto::v1::{ScrollEvent, ScrollUnit};

use super::translate::DISCRETE_PER_NOTCH;
use crate::input::scroll::{MAX_LINES_PER_EVENT, SCROLL_PIXELS_PER_LINE};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum ScrollRequest {
    /// 120 per wheel notch.
    Discrete { x: i32, y: i32 },
    /// Logical pixels.
    Smooth { x: f32, y: f32 },
}

/// A peer's scroll event as a libei request. Non-finite values become zero and
/// one event is bounded to [`MAX_LINES_PER_EVENT`] notches, as on X11.
#[allow(
    clippy::cast_possible_truncation,
    reason = "bounded by MAX_LINES_PER_EVENT"
)]
pub(super) fn scroll_request(event: &ScrollEvent) -> Option<ScrollRequest> {
    let bounded = |value: f32, limit: f32| {
        if value.is_finite() {
            value.clamp(-limit, limit)
        } else {
            0.0
        }
    };
    let max_lines = MAX_LINES_PER_EVENT as f32;
    let request = match event.unit() {
        ScrollUnit::Unspecified => return None,
        ScrollUnit::Line => ScrollRequest::Discrete {
            x: (bounded(event.dx, max_lines) * DISCRETE_PER_NOTCH).round() as i32,
            y: (bounded(event.dy, max_lines) * DISCRETE_PER_NOTCH).round() as i32,
        },
        ScrollUnit::Pixel => {
            let max_pixels = max_lines * SCROLL_PIXELS_PER_LINE as f32;
            ScrollRequest::Smooth {
                x: bounded(event.dx, max_pixels),
                y: bounded(event.dy, max_pixels),
            }
        }
    };
    Some(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_request_hostile_values_are_bounded_or_dropped() {
        // Arrange
        let huge_lines = ScrollEvent {
            dx: f32::NAN,
            dy: 1e30,
            unit: ScrollUnit::Line.into(),
        };
        let one_notch = ScrollEvent {
            dx: 0.0,
            dy: 1.0,
            unit: ScrollUnit::Line.into(),
        };
        let unspecified = ScrollEvent {
            dx: 1.0,
            dy: 1.0,
            unit: ScrollUnit::Unspecified.into(),
        };

        // Act
        let requests = [huge_lines, one_notch, unspecified].map(|event| scroll_request(&event));

        // Assert
        assert_eq!(
            requests,
            [
                Some(ScrollRequest::Discrete { x: 0, y: 12_000 }),
                Some(ScrollRequest::Discrete { x: 0, y: 120 }),
                None
            ]
        );
    }
}
