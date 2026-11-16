// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure screen arithmetic for the macOS feedback windows.
//!
//! The daemon speaks top-left desktop coordinates; `AppKit` puts the origin at
//! the bottom-left of the primary screen. Everything here is in `AppKit` space
//! unless a name says `desktop`. It compiles on every platform so the
//! decisions can be tested without a Mac.

pub(super) use super::look::Rect;
use super::look::edge_strip;
use a7n_proto::v1::Edge;

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

/// The glow strip `depth` points thick along `edge` of `screen`. `AppKit`'s y
/// axis points up, so the top edge lies at the far end of y.
pub(super) fn glow_rect(screen: Rect, edge: Edge, depth: f64) -> Option<Rect> {
    let edge = match edge {
        Edge::Top => Edge::Bottom,
        Edge::Bottom => Edge::Top,
        other => other,
    };
    edge_strip(screen, edge, depth)
}

/// A top-left desktop point in `AppKit` space.
pub(super) fn desktop_to_appkit(x: i32, y: i32, primary_height: f64) -> (f64, f64) {
    (f64::from(x), primary_height - f64::from(y))
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
    fn glow_rect_top_and_bottom_follow_appkit_y_up() {
        // Arrange
        let depth = 10.0;

        // Act
        let top = glow_rect(SCREEN, Edge::Top, depth);
        let bottom = glow_rect(SCREEN, Edge::Bottom, depth);

        // Assert
        assert_eq!(top.map(|rect| rect.y), Some(590.0));
        assert_eq!(bottom.map(|rect| rect.y), Some(0.0));
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
    fn desktop_to_appkit_point_is_flipped_against_primary_height() {
        // Arrange
        let primary_height = 900.0;

        // Act
        let point = desktop_to_appkit(100, 200, primary_height);

        // Assert
        assert_eq!(point, (100.0, 700.0));
    }
}
