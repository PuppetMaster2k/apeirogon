// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared feedback state machine. Native event loops supply monotonic elapsed time.
//!
//! Every frame is a pure function of its inputs; time comes in as a parameter.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use super::raster::{self, EdgeEffect, Frame};
use super::{Look, Style};

pub(super) const PLAY_DURATION: Duration = Duration::from_millis(650);
pub(super) const FADE_DURATION: Duration = Duration::from_millis(140);
const REDUCED_DURATION: Duration = Duration::from_millis(200);
const QUARTER: u16 = 250;
pub(super) const SHUDDER_DURATION: Duration = Duration::from_millis(250);
const DEFAULT_REFRESH_HZ: f64 = 60.0;
const MIN_REFRESH_HZ: f64 = 30.0;
const MAX_REFRESH_HZ: f64 = 240.0;

pub(super) fn departure_duration(style: Style, animate: bool) -> Duration {
    if !animate {
        REDUCED_DURATION
    } else if style == Style::Flow {
        super::flow::FINISH_DURATION
    } else if style.animated() {
        PLAY_DURATION
    } else {
        FADE_DURATION
    }
}

pub(super) fn arrival_duration(style: Style, animate: bool) -> Duration {
    if !animate {
        REDUCED_DURATION
    } else if style == Style::Flow {
        super::flow::ARRIVAL_DURATION
    } else {
        PLAY_DURATION
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

#[derive(Clone, Copy, Default)]
pub(super) struct Motion {
    pub seconds: f32,
    pub tick: f32,
    pub finish: Option<(End, f32)>,
}

struct Departure {
    effect: EdgeEffect,
    started: Duration,
    tick: Option<Duration>,
    ended: Option<(End, Duration)>,
}

#[derive(Default)]
pub(super) struct Timeline {
    departure: Option<Departure>,
    arrival: Option<(Look, Duration)>,
    /// Set once an effect panicked; every later frame is the plain glow.
    fallback: bool,
}

impl Timeline {
    pub fn push(&mut self, effect: EdgeEffect, now: Duration) {
        let Some(departure) = self.departure.as_mut().filter(|d| {
            d.ended.is_none() && d.effect.edge == effect.edge && d.effect.look == effect.look
        }) else {
            self.departure = Some(Departure {
                effect,
                started: now,
                tick: (effect.progress >= QUARTER).then_some(now),
                ended: None,
            });
            return;
        };
        if effect.progress / QUARTER > departure.effect.progress / QUARTER {
            departure.tick = Some(now);
        }
        departure.effect = effect;
    }

    pub fn end(&mut self, end: End, now: Duration) {
        if let Some(departure) = &mut self.departure {
            // Repeated clears must not restart a finish that is already playing.
            if departure.ended.is_none() {
                departure.ended = Some((end, now));
                if matches!(end, End::Crossed) {
                    departure.effect.progress = 1000;
                }
            }
        }
    }

    pub fn land(&mut self, look: Look, now: Duration) {
        self.arrival = Some((look, now));
    }

    pub fn edge_playing(&self, animate: bool, now: Duration) -> bool {
        self.departure.as_ref().is_some_and(|d| {
            let style = if self.fallback {
                Style::Glow
            } else {
                d.effect.look.style
            };
            d.ended.is_some()
                || (animate
                    && style.animated()
                    && (matches!(style, Style::Sparks | Style::Flow)
                        || d.tick
                            .is_some_and(|tick| now.saturating_sub(tick) < SHUDDER_DURATION)))
        })
    }

    pub fn landing_visible(&self) -> bool {
        self.arrival.is_some()
    }

    pub fn render_edge(&mut self, frame: &mut Frame, now: Duration) -> bool {
        let Some(departure) = &self.departure else {
            return false;
        };
        let mut effect = departure.effect;
        if self.fallback {
            effect.look.style = Style::Glow;
        }
        let mut motion = Motion {
            seconds: now.saturating_sub(departure.started).as_secs_f32(),
            tick: departure.tick.map_or(0.0, |tick| {
                (1.0 - now.saturating_sub(tick).as_secs_f32() / SHUDDER_DURATION.as_secs_f32())
                    .max(0.0)
            }),
            finish: None,
        };
        let opacity = frame.opacity;
        if let Some((end, started)) = departure.ended {
            let duration = departure_duration(effect.look.style, frame.animate);
            let phase = now.saturating_sub(started).as_secs_f32() / duration.as_secs_f32();
            if phase >= 1.0 {
                self.departure = None;
                return false;
            }
            if effect.look.style != Style::Flow {
                motion.seconds = started.saturating_sub(departure.started).as_secs_f32();
            }
            motion.finish = Some((end, phase));
            frame.opacity *= f64::from(1.0 - phase);
            if matches!(end, End::Abandoned) {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                {
                    effect.progress = (f32::from(effect.progress) * (1.0 - phase)) as u16;
                }
            }
        }
        self.guarded(effect.look.style, || {
            raster::render_departure(frame, effect, motion);
        });
        frame.opacity = opacity;
        true
    }

    pub fn render_landing(&mut self, frame: &mut Frame, now: Duration) -> bool {
        let Some((mut look, started)) = self.arrival else {
            return false;
        };
        if self.fallback {
            look.style = Style::Glow;
        }
        let duration = arrival_duration(look.style, frame.animate);
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
