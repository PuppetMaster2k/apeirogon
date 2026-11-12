// SPDX-License-Identifier: GPL-3.0-or-later

//! Local edge appearance and shared beam geometry in top-left coordinates.

use std::time::Duration;

use a7n_proto::v1::Edge;
use serde::{Deserialize, Serialize};

#[cfg(not(target_os = "macos"))]
use crate::input::Bounds;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Style {
    #[default]
    Glow,
    Beam,
    Aperture,
    Sparks,
    Ripple,
}

impl Style {
    pub(super) const fn animated(self) -> bool {
        matches!(self, Self::Aperture | Self::Sparks | Self::Ripple)
    }
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
    #[cfg(not(target_os = "macos"))]
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Look {
    pub style: Style,
    #[serde(rename = "landing_style")]
    pub landing: Landing,
    pub color: Color,
    pub size: Size,
}

pub(super) const BEAM_THICKNESS: f64 = 3.0;
const PROGRESS_FULL: f64 = 1000.0;

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

pub(super) fn beam_rect(
    area: Rect,
    edge: Edge,
    pointer: (f64, f64),
    progress: u16,
    thickness: f64,
) -> Option<Rect> {
    let horizontal = match edge {
        Edge::Top | Edge::Bottom => true,
        Edge::Left | Edge::Right => false,
        Edge::Unspecified => return None,
    };
    let (start, span, along) = if horizontal {
        (area.x, area.width, pointer.0)
    } else {
        (area.y, area.height, pointer.1)
    };
    if span <= 0.0 || thickness <= 0.0 {
        return None;
    }
    let length = (span * f64::from(progress) / PROGRESS_FULL).clamp(1.0_f64.min(span), span);
    let from = (along - length / 2.0).clamp(start, start + span - length);
    let thickness = thickness.min(if horizontal { area.height } else { area.width });
    Some(match edge {
        Edge::Top => Rect {
            x: from,
            y: area.y,
            width: length,
            height: thickness,
        },
        Edge::Bottom => Rect {
            x: from,
            y: area.y + area.height - thickness,
            width: length,
            height: thickness,
        },
        Edge::Left => Rect {
            x: area.x,
            y: from,
            width: thickness,
            height: length,
        },
        Edge::Right => Rect {
            x: area.x + area.width - thickness,
            y: from,
            width: thickness,
            height: length,
        },
        Edge::Unspecified => return None,
    })
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

    #[test]
    fn beam_rect_progress_grows_along_pushed_edge() {
        // Arrange
        let area = Rect {
            x: -100.0,
            y: 20.0,
            width: 1000.0,
            height: 800.0,
        };
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            // Act
            let small = beam_rect(area, edge, (400.0, 420.0), 250, BEAM_THICKNESS).unwrap();
            let large = beam_rect(area, edge, (400.0, 420.0), 750, BEAM_THICKNESS).unwrap();

            // Assert
            for rect in [small, large] {
                assert!(rect.x >= area.x && rect.y >= area.y);
                assert!(rect.x + rect.width <= area.x + area.width);
                assert!(rect.y + rect.height <= area.y + area.height);
                match edge {
                    Edge::Top => assert_eq!(rect.y, area.y),
                    Edge::Bottom => assert_eq!(rect.y + rect.height, area.y + area.height),
                    Edge::Left => assert_eq!(rect.x, area.x),
                    Edge::Right => assert_eq!(rect.x + rect.width, area.x + area.width),
                    Edge::Unspecified => unreachable!(),
                }
            }
            if matches!(edge, Edge::Top | Edge::Bottom) {
                assert_eq!((small.width, large.width), (250.0, 750.0));
            } else {
                assert_eq!((small.height, large.height), (200.0, 600.0));
            }
        }
    }
}
