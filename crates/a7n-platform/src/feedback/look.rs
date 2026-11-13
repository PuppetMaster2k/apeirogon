// SPDX-License-Identifier: GPL-3.0-or-later

//! Local edge appearance and shared beam geometry in top-left coordinates.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::parameters;

#[cfg(not(target_os = "macos"))]
use crate::input::Bounds;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Style {
    Sparks,
    Waves,
    Clouds,
    Sunrise,
    Aurora,
    Arcs,
    Ferrofluid,
    #[default]
    #[serde(other)]
    Membrane,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Landing {
    #[default]
    Ring,
    Pulse,
    Dot,
}

impl Landing {
    /// Radius relative to the backend's full ring, and opacity, at this instant.
    pub(super) fn frame(self, elapsed: Duration, duration: Duration, animate: bool) -> (f64, f64) {
        const PULSE_START: f64 = 0.2;
        const DOT_RADIUS: f64 = 0.35;
        let progress = (elapsed.as_secs_f64() / duration.as_secs_f64()).clamp(0.0, 1.0);
        let motion = if animate { progress } else { 0.0 };
        let radius = match self {
            Self::Ring => 1.0,
            Self::Pulse if !animate => 1.0,
            Self::Pulse => PULSE_START + (1.0 - PULSE_START) * motion,
            Self::Dot => DOT_RADIUS * (1.0 - motion),
        };
        let alpha = if progress >= 1.0 {
            0.0
        } else if animate {
            1.0 - progress
        } else {
            1.0
        };
        (radius, alpha)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Color {
    #[default]
    Accent,
    Green,
    Orange,
    Pink,
    Ibtesam,
    Lunada,
    Stripe,
    Ali,
    Alihossein,
    Transfile,
    ElectricViolet,
    Martini,
    Superman,
    EdsSunsetGradient,
    SublimeLight,
    Argon,
    #[serde(alias = "jshine")]
    JShine,
    Flare,
    UltraVoilet,
    KyooPal,
    KyeMeh,
    Wiretap,
    Rastafari,
    SinCityRed,
    AzurLane,
    WitchingHour,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Size {
    Small,
    #[default]
    Medium,
    Large,
}

impl Size {
    #[cfg(all(unix, not(target_os = "macos")))]
    // Physical pixel dimensions are rounded after applying the local scale.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub(super) fn pixels(self, base: u32) -> u32 {
        (f64::from(base) * self.multiplier()).round().max(1.0) as u32
    }

    #[must_use]
    pub(super) const fn multiplier(self) -> f64 {
        match self {
            Self::Small => 0.5,
            Self::Medium => 1.0,
            Self::Large => 2.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Look {
    pub style: Style,
    #[serde(rename = "landing_style")]
    pub landing: Landing,
    pub color: Color,
    pub size: Size,
    pub depth: f32,
    pub radius: f32,
    pub speed: f32,
    pub frequency: f32,
    pub intensity: f32,
    #[serde(skip)]
    pub show_edge: bool,
    #[serde(skip)]
    pub show_marker: bool,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            style: Style::default(),
            landing: Landing::default(),
            color: Color::default(),
            size: Size::default(),
            depth: parameters::DEPTH.default,
            radius: parameters::RADIUS.default,
            speed: parameters::SPEED.default,
            frequency: parameters::FREQUENCY.default,
            intensity: parameters::INTENSITY.default,
            show_edge: true,
            show_marker: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[cfg(not(target_os = "macos"))]
impl Rect {
    #[cfg(all(unix, not(target_os = "macos")))]
    pub(super) fn from_bounds(bounds: Bounds) -> Self {
        Self {
            x: f64::from(bounds.x),
            y: f64::from(bounds.y),
            width: f64::from(bounds.width),
            height: f64::from(bounds.height),
        }
    }

    // Desktop rectangles are bounded by the OS; integer backends need pixel rounding.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub(super) fn bounds(self) -> Bounds {
        Bounds {
            x: self.x.round() as i32,
            y: self.y.round() as i32,
            width: self.width.round().max(1.0) as u32,
            height: self.height.round().max(1.0) as u32,
        }
    }
}

/// Keep expanding arrivals on the landing monitor, including points at its edge.
pub(super) fn landing_rect(area: Rect, point: (f64, f64), diameter: f64) -> Rect {
    let width = diameter.min(area.width);
    let height = diameter.min(area.height);
    Rect {
        x: (point.0 - width / 2.0).clamp(area.x, area.x + area.width - width),
        y: (point.1 - height / 2.0).clamp(area.y, area.y + area.height - height),
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_elapsed_pulse_grows_and_dot_shrinks() {
        // Arrange
        let duration = Duration::from_millis(400);
        for (style, grows) in [(Landing::Pulse, true), (Landing::Dot, false)] {
            // Act
            let start = style.frame(Duration::ZERO, duration, true);
            let later = style.frame(duration / 2, duration, true);
            let reduced = style.frame(duration / 2, duration, false);

            // Assert
            assert!(if grows {
                later.0 > start.0
            } else {
                later.0 < start.0
            });
            assert!(later.1 < start.1);
            assert_eq!(reduced, style.frame(Duration::ZERO, duration, false));
            assert_eq!(style.frame(duration, duration, true).1, 0.0);
        }
    }
}
