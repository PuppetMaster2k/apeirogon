// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared feedback state machine. Native event loops supply monotonic elapsed time.
//!
//! Native loops own state and supply elapsed time; particles persist between frames.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use super::raster::{self, EdgeEffect, Frame};
use super::{Look, Style};

pub(super) const MARKER_DURATION: Duration = Duration::from_millis(650);
pub(super) const BLOOM_DURATION: Duration = Duration::from_millis(800);
pub(super) const REDUCED_DURATION: Duration = Duration::from_millis(200);
const DEFAULT_REFRESH_HZ: f64 = 60.0;
const MIN_REFRESH_HZ: f64 = 30.0;
const MAX_REFRESH_HZ: f64 = 240.0;

pub(super) fn departure_duration(_style: Style, _animate: bool) -> Duration {
    Duration::from_millis(150)
}
pub(super) fn arrival_duration(_style: Style, animate: bool) -> Duration {
    (if animate {
        BLOOM_DURATION
    } else {
        REDUCED_DURATION
    }) + marker_duration(animate)
}

fn marker_duration(animate: bool) -> Duration {
    if animate {
        MARKER_DURATION
    } else {
        REDUCED_DURATION
    }
}

/// The pause between frames for a display refreshing at `refresh_hz`. Elapsed
/// time drives the animation, so the rate only changes how smooth it looks.
pub(super) fn frame_interval(refresh_hz: Option<f64>) -> Duration {
    let hz = refresh_hz
        .filter(|hz| hz.is_finite() && *hz > 0.0)
        .unwrap_or(DEFAULT_REFRESH_HZ)
        .clamp(MIN_REFRESH_HZ, MAX_REFRESH_HZ);
    Duration::from_secs_f64(1.0 / hz)
}

#[derive(Clone, Copy)]
pub(super) enum End {
    Crossed,
    Abandoned,
}

struct Departure {
    effect: EdgeEffect,
    started: Duration,
    ended: Option<Duration>,
}

#[derive(Default)]
pub(super) struct Timeline {
    departure: Option<Departure>,
    arrival: Option<(Look, Duration)>,
    /// A failing renderer is disabled for this feedback thread.
    fallback: bool,
}

impl Timeline {
    pub fn push(&mut self, effect: EdgeEffect, now: Duration) {
        let Some(departure) = self.departure.as_mut().filter(|d| {
            d.ended.is_none()
                && d.effect.edge == effect.edge
                && d.effect.look == effect.look
                && d.effect.along.origin == effect.along.origin
        }) else {
            self.departure = Some(Departure {
                effect,
                started: now,
                ended: None,
            });
            return;
        };
        let mut effect = effect;
        effect.along.push = departure.effect.along.push;
        departure.effect = effect;
    }

    pub fn end(&mut self, end: End, now: Duration) {
        if let Some(departure) = &mut self.departure {
            // Repeated clears must not restart a finish that is already playing.
            if departure.ended.is_none() {
                departure.ended = Some(now);
                if matches!(end, End::Crossed) {
                    departure.effect.progress = 1000;
                }
            }
        }
    }

    pub fn land(&mut self, look: Look, now: Duration) {
        self.arrival = Some((look, now));
    }

    pub fn edge_playing(&self, animate: bool, _now: Duration) -> bool {
        !self.fallback
            && self
                .departure
                .as_ref()
                .is_some_and(|d| d.ended.is_some() || animate)
    }

    pub fn landing_visible(&self) -> bool {
        self.arrival.is_some()
    }

    pub fn render_edge(&mut self, frame: &mut Frame, now: Duration) -> bool {
        let Some(departure) = &self.departure else {
            return false;
        };
        if self.fallback {
            frame.pixels.fill(0);
            return false;
        }
        let effect = departure.effect;
        let seconds = now.saturating_sub(departure.started).as_secs_f32();
        let opacity = frame.opacity;
        if let Some(started) = departure.ended {
            let duration = departure_duration(effect.look.style, frame.animate);
            let phase = now.saturating_sub(started).as_secs_f32() / duration.as_secs_f32();
            if phase >= 1.0 {
                self.departure = None;
                return false;
            }
            frame.opacity *= f64::from(1.0 - phase);
        }
        self.guarded(effect.look.style, || {
            raster::render_departure(frame, effect, seconds);
        });
        frame.opacity = opacity;
        true
    }

    pub fn render_landing(&mut self, frame: &mut Frame, now: Duration) -> bool {
        let Some((look, started)) = self.arrival else {
            return false;
        };
        if self.fallback {
            self.arrival = None;
            frame.pixels.fill(0);
            return false;
        }
        let duration = if look.show_edge && frame.arrival.is_some_and(|a| a.edge.is_some()) {
            arrival_duration(look.style, frame.animate).saturating_sub(if look.show_marker {
                Duration::ZERO
            } else {
                marker_duration(frame.animate)
            })
        } else {
            marker_duration(frame.animate)
        };
        let elapsed = now.saturating_sub(started);
        if elapsed >= duration {
            self.arrival = None;
            return false;
        }
        self.guarded(look.style, || {
            raster::render_landing(frame, look, elapsed, duration);
        });
        true
    }

    /// Rendering must not take the feedback thread down. This relies on
    /// unwinding, so the release profile must not set `panic = "abort"`.
    fn guarded(&mut self, style: Style, render: impl FnOnce()) {
        if catch_unwind(AssertUnwindSafe(render)).is_err() && !self.fallback {
            eprintln!("kind=feedback_effect_failed style={style:?}");
            self.fallback = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use a7n_proto::v1::Edge;

    use super::raster::Along;
    use super::*;

    #[test]
    fn frame_interval_refresh_rates_clamp_and_default() {
        // Arrange
        let cases = [
            (None, 60.0),
            (Some(0.0), 60.0),
            (Some(f64::NAN), 60.0),
            (Some(60.0), 60.0),
            (Some(144.0), 144.0),
            (Some(1000.0), MAX_REFRESH_HZ),
            (Some(10.0), MIN_REFRESH_HZ),
        ];

        for (refresh_hz, expected_hz) in cases {
            // Act
            let interval = frame_interval(refresh_hz);

            // Assert
            let expected = Duration::from_secs_f64(1.0 / expected_hz);
            assert!(
                interval.abs_diff(expected) < Duration::from_micros(1),
                "{refresh_hz:?}"
            );
        }
    }

    #[test]
    fn render_edge_crossed_plays_finish_then_ends() {
        // Arrange
        let mut timeline = Timeline::default();
        let mut frame = Frame::default();
        frame.resize(8, 8);
        frame.animate = true;
        frame.opacity = 1.0;
        let effect = EdgeEffect {
            look: Look::default(),
            edge: Edge::Top,
            progress: 500,
            along: Along {
                origin: (0.0, 0.0),
                start: 0.0,
                span: 8.0,
                push: 4.0,
            },
        };
        timeline.push(effect, Duration::ZERO);
        timeline.end(End::Crossed, Duration::from_millis(100));

        // Act
        let during = timeline.render_edge(&mut frame, Duration::from_millis(150));
        let after = timeline.render_edge(&mut frame, Duration::from_millis(300));

        // Assert
        assert_eq!((during, after), (true, false));
        assert!(timeline.departure.is_none());
    }
}
