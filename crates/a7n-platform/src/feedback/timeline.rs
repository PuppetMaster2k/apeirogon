// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared feedback state machine. Native event loops supply monotonic elapsed time.

use std::time::Duration;

use super::raster::{self, EdgeEffect, Frame};
use super::{Look, Style};

const PLAY_DURATION: Duration = Duration::from_millis(650);
const FADE_DURATION: Duration = Duration::from_millis(140);
const REDUCED_DURATION: Duration = Duration::from_millis(200);
const QUARTER: u16 = 250;
const SHUDDER_DURATION: Duration = Duration::from_millis(250);

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
            d.ended.is_some()
                || (animate
                    && d.effect.look.style.animated()
                    && (d.effect.look.style == Style::Sparks
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
            let duration = if !frame.animate {
                REDUCED_DURATION
            } else if effect.look.style.animated() {
                PLAY_DURATION
            } else {
                FADE_DURATION
            };
            let phase = now.saturating_sub(started).as_secs_f32() / duration.as_secs_f32();
            if phase >= 1.0 {
                self.departure = None;
                return false;
            }
            motion.seconds = started.saturating_sub(departure.started).as_secs_f32();
            motion.finish = Some((end, phase));
            frame.opacity *= f64::from(1.0 - phase);
            if matches!(end, End::Abandoned) {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                {
                    effect.progress = (f32::from(effect.progress) * (1.0 - phase)) as u16;
                }
            }
        }
        raster::render_departure(frame, effect, motion);
        frame.opacity = opacity;
        true
    }

    pub fn render_landing(&mut self, frame: &mut Frame, now: Duration) -> bool {
        let Some((look, started)) = self.arrival else {
            return false;
        };
        let duration = if frame.animate {
            PLAY_DURATION
        } else {
            REDUCED_DURATION
        };
        let elapsed = now.saturating_sub(started);
        if elapsed >= duration {
            self.arrival = None;
            return false;
        }
        raster::render_landing(frame, look, elapsed, duration);
        true
    }
}
