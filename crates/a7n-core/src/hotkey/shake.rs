// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub const SHAKE_DEFAULT: u8 = 6;
pub const SHAKE_MIN: u8 = 3;
pub const SHAKE_MAX: u8 = 10;
pub const SHAKE_MIN_TRAVEL: u32 = 80;
pub const SHAKE_WINDOW: Duration = Duration::from_millis(600);

/// Horizontal reversals measured using time supplied by the input owner.
#[derive(Debug)]
pub struct ShakeDetector {
    shakes: u8,
    direction: i32,
    travel: u32,
    reversals: VecDeque<Instant>,
}

impl ShakeDetector {
    #[must_use]
    pub fn new(shakes: u8) -> Self {
        Self {
            shakes,
            direction: 0,
            travel: 0,
            reversals: VecDeque::with_capacity(usize::from(shakes)),
        }
    }

    pub fn on_motion(&mut self, dx: i32, now: Instant) -> bool {
        if self.shakes == 0 || dx == 0 {
            return false;
        }
        while self
            .reversals
            .front()
            .is_some_and(|first| now.saturating_duration_since(*first) > SHAKE_WINDOW)
        {
            self.reversals.pop_front();
        }
        let direction = dx.signum();
        if direction == self.direction {
            self.travel = self.travel.saturating_add(dx.unsigned_abs());
            return false;
        }
        if self.travel >= SHAKE_MIN_TRAVEL {
            self.reversals.push_back(now);
        }
        self.direction = direction;
        self.travel = dx.unsigned_abs();
        if self.reversals.len() < usize::from(self.shakes) {
            return false;
        }
        self.reversals.clear();
        self.direction = 0;
        self.travel = 0;
        true
    }
}

#[cfg(test)]
mod tests;
