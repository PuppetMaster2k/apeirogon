// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure screen arithmetic for the macOS feedback windows.
//!
//! The daemon speaks top-left desktop coordinates; `AppKit` puts the origin at
//! the bottom-left of the primary screen. Everything here is in `AppKit` space
//! unless a name says `desktop`. It compiles on every platform so the
//! decisions can be tested without a Mac.

use super::Size;
pub(super) use super::look::Rect;
use a7n_proto::v1::Edge;

/// `Command::Edge::progress` at full resistance, per-mille.
const PROGRESS_FULL: u16 = 1000;
/// Glow thickness in points at the first push.
const GLOW_MIN_THICKNESS: f64 = 3.0;
/// Glow thickness in points at full resistance.
const GLOW_MAX_THICKNESS: f64 = 28.0;
/// Outer diameter of the landing ring in points.
pub(super) const LANDING_DIAMETER: f64 = 64.0;

impl Rect {
    fn contains(&self, x: f64, y: f64) -> bool {
        (self.x..=self.x + self.width).contains(&x) && (self.y..=self.y + self.height).contains(&y)
    }
}

/// The screen holding the point, or the first (primary) one as a fallback.
pub(super) fn screen_containing(screens: &[Rect], x: f64, y: f64) -> Option<Rect> {
    screens
        .iter()
        .find(|screen| screen.contains(x, y))
        .or_else(|| screens.first())
        .copied()
}

/// The strip along `edge` of `screen`, thicker the more of the resistance is
/// built up. `None` for an unspecified edge.
pub(super) fn glow_rect(screen: Rect, edge: Edge, progress: u16, size: Size) -> Option<Rect> {
    let fraction = f64::from(progress.min(PROGRESS_FULL)) / f64::from(PROGRESS_FULL);
    let thickness = (GLOW_MIN_THICKNESS + (GLOW_MAX_THICKNESS - GLOW_MIN_THICKNESS) * fraction)
        * size.multiplier();
    let Rect {
        x,
        y,
        width,
        height,
    } = screen;
    match edge {
        Edge::Left => Some(Rect {
            x,
            y,
            width: thickness,
            height,
        }),
        Edge::Right => Some(Rect {
            x: x + width - thickness,
            y,
            width: thickness,
            height,
        }),
        Edge::Top => Some(Rect {
            x,
            y: y + height - thickness,
            width,
            height: thickness,
        }),
        Edge::Bottom => Some(Rect {
            x,
            y,
            width,
            height: thickness,
        }),
        Edge::Unspecified => None,
    }
}

/// The landing ring centred on a desktop point.
pub(super) fn landing_rect(desktop_x: i32, desktop_y: i32, primary_height: f64) -> Rect {
    let centre_x = f64::from(desktop_x);
    let centre_y = primary_height - f64::from(desktop_y);
    Rect {
        x: centre_x - LANDING_DIAMETER / 2.0,
        y: centre_y - LANDING_DIAMETER / 2.0,
        width: LANDING_DIAMETER,
        height: LANDING_DIAMETER,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1000.0,
        height: 600.0,
    };

    #[test]
    fn glow_rect_edges_hug_their_side_and_grow_with_progress() {
        // Arrange
        let cases = [
            (
                Edge::Left,
                0,
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: GLOW_MIN_THICKNESS,
                    height: 600.0,
                },
            ),
            (
                Edge::Right,
                PROGRESS_FULL,
                Rect {
                    x: 1000.0 - GLOW_MAX_THICKNESS,
                    y: 0.0,
                    width: GLOW_MAX_THICKNESS,
                    height: 600.0,
                },
            ),
            (
                Edge::Top,
                PROGRESS_FULL * 2,
                Rect {
                    x: 0.0,
                    y: 600.0 - GLOW_MAX_THICKNESS,
                    width: 1000.0,
                    height: GLOW_MAX_THICKNESS,
                },
            ),
            (
                Edge::Bottom,
                0,
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 1000.0,
                    height: GLOW_MIN_THICKNESS,
                },
            ),
        ];

        for (edge, progress, expected) in cases {
            // Act
            let rect = glow_rect(SCREEN, edge, progress, Size::Medium);

            // Assert
            assert_eq!(rect, Some(expected), "{edge:?} at {progress}");
        }
    }

    #[test]
    fn screen_containing_point_off_every_screen_falls_back_to_primary() {
        // Arrange
        let right = Rect {
            x: 1000.0,
            ..SCREEN
        };
        let screens = [SCREEN, right];

        // Act
        let inside = screen_containing(&screens, 1500.0, 10.0);
        let outside = screen_containing(&screens, -50.0, 10.0);

        // Assert
        assert_eq!((inside, outside), (Some(right), Some(SCREEN)));
    }

    #[test]
    fn landing_rect_desktop_point_is_flipped_into_appkit_space() {
        // Arrange
        let primary_height = 900.0;

        // Act
        let rect = landing_rect(100, 200, primary_height);

        // Assert
        let half = LANDING_DIAMETER / 2.0;
        assert_eq!((rect.x, rect.y), (100.0 - half, 700.0 - half));
    }
}
