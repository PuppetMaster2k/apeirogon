// SPDX-License-Identifier: GPL-3.0-or-later

//! Local safety hotkeys. Time is supplied by the input owner.

use std::time::{Duration, Instant};

use crate::modifiers::KEYBOARD_PAGE;

const MODIFIER_START: u32 = 0xe0;
const MODIFIER_COUNT: u32 = 8;
const KEY_COUNT: usize = 256;
mod keys;
mod shake;
mod trigger;

use keys::key_usage;
pub use keys::trigger_name;
pub use shake::{
    SHAKE_DEFAULT, SHAKE_MAX, SHAKE_MIN, SHAKE_MIN_TRAVEL, SHAKE_WINDOW, ShakeDetector,
};
pub use trigger::{DoubleTap, HOLD_DEFAULT, HOLD_MAX, HOLD_MIN, TriggerMode};

const DIGIT_ONE: u32 = KEYBOARD_PAGE | 0x1e;
const DIGIT_NINE: u32 = KEYBOARD_PAGE | 0x26;
pub const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(400);

fn modifier_bit(usage: u32) -> Option<u8> {
    let offset = usage.checked_sub(KEYBOARD_PAGE | MODIFIER_START)?;
    (offset < MODIFIER_COUNT).then(|| 1 << offset)
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct HotkeyError(String);

/// Either side of each requested modifier matches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods(u8);

impl Mods {
    fn token(token: &str) -> Option<Self> {
        match token {
            "ctrl" => Some(Self(1)),
            "shift" => Some(Self(2)),
            "alt" => Some(Self(4)),
            "meta" => Some(Self(8)),
            _ => None,
        }
    }

    /// # Errors
    /// Rejects empty, repeated or unknown modifier tokens.
    pub fn parse(text: &str) -> Result<Self, HotkeyError> {
        let mut mods = Self::default();
        for token in text.split('+') {
            let token = token.trim().to_ascii_lowercase();
            let value = Self::token(&token)
                .ok_or_else(|| HotkeyError(format!("unknown modifier '{token}'")))?;
            mods.add(value, &token)?;
        }
        Ok(mods)
    }

    fn add(&mut self, other: Self, token: &str) -> Result<(), HotkeyError> {
        if self.0 & other.0 != 0 {
            return Err(HotkeyError(format!("repeated modifier '{token}'")));
        }
        self.0 |= other.0;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub mods: Mods,
    pub key: u32,
}

impl Chord {
    /// # Errors
    /// Rejects unknown tokens, repeated modifiers and anything but one key.
    pub fn parse(text: &str) -> Result<Self, HotkeyError> {
        let mut mods = Mods::default();
        let mut key = None;
        for token in text.split('+') {
            let token = token.trim().to_ascii_lowercase();
            if let Some(value) = Mods::token(&token) {
                mods.add(value, &token)?;
            } else {
                let usage = key_usage(&token)
                    .filter(|usage| modifier_bit(*usage).is_none())
                    .ok_or_else(|| HotkeyError(format!("unknown key '{token}'")))?;
                if key.replace(usage).is_some() {
                    return Err(HotkeyError(format!("multiple keys at '{token}'")));
                }
            }
        }
        Ok(Self {
            mods,
            key: key.ok_or_else(|| HotkeyError(format!("missing key in {text}")))?,
        })
    }
    /// The one-based jump number when this chord uses the jump modifiers.
    #[must_use]
    pub fn jump_target(self, jump_mods: Mods) -> Option<u8> {
        if self.mods != jump_mods || !(DIGIT_ONE..=DIGIT_NINE).contains(&self.key) {
            return None;
        }
        u8::try_from(self.key - DIGIT_ONE + 1).ok()
    }
}

#[derive(Debug, Clone)]
pub struct Bindings {
    pub double_tap: DoubleTap,
    pub trigger_mode: TriggerMode,
    pub hold: Duration,
    pub home: Vec<Chord>,
    pub lock: Chord,
    pub jump_mods: Mods,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hotkey {
    Home,
    Lock,
    Jump(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verdict {
    pub forward: bool,
    pub action: Option<Hotkey>,
}

#[derive(Debug)]
pub struct HotkeyMatcher {
    bindings: Bindings,
    held_mods: u8,
    pressed: [bool; KEY_COUNT],
    consumed: [bool; KEY_COUNT],
    first_tap: Option<Instant>,
    tap_released: bool,
    armed: Option<(u32, Instant)>,
}

impl HotkeyMatcher {
    #[must_use]
    pub fn new(bindings: Bindings) -> Self {
        Self {
            bindings,
            held_mods: 0,
            pressed: [false; KEY_COUNT],
            consumed: [false; KEY_COUNT],
            first_tap: None,
            tap_released: false,
            armed: None,
        }
    }

    pub fn on_key(&mut self, usage: u32, down: bool, now: Instant) -> Verdict {
        let mut verdict = Verdict {
            forward: true,
            action: None,
        };
        let Some(index) = usage
            .checked_sub(KEYBOARD_PAGE)
            .and_then(|key| usize::try_from(key).ok())
            .filter(|key| *key < KEY_COUNT)
        else {
            if down {
                self.first_tap = None;
                self.armed = None;
            }
            return verdict;
        };
        let was_down = std::mem::replace(&mut self.pressed[index], down);
        let modifier = modifier_bit(usage);
        if let Some(bit) = modifier {
            if down {
                self.held_mods |= bit;
            } else {
                self.held_mods &= !bit;
            }
        }
        verdict.action = self.trigger(usage, down, was_down, now);
        if modifier.is_some() {
            return verdict;
        }
        if self.consumed[index] {
            self.consumed[index] = down;
            verdict.forward = false;
        } else if down && !was_down {
            verdict.action = verdict.action.or_else(|| self.chord(usage));
            verdict.forward = verdict.action.is_none();
            self.consumed[index] = !verdict.forward;
        }
        verdict
    }

    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        self.armed.map(|(_, deadline)| deadline)
    }

    pub fn poll(&mut self, now: Instant) -> Option<Hotkey> {
        if self.next_deadline().is_none_or(|deadline| now < deadline) {
            return None;
        }
        if let Some((usage, _)) = self.armed.take() {
            if let Ok(index) = usize::try_from(usage - KEYBOARD_PAGE) {
                if let Some(consumed) = self.consumed.get_mut(index) {
                    *consumed = true;
                }
            }
        }
        self.first_tap = None;
        Some(Hotkey::Home)
    }

    fn trigger(&mut self, usage: u32, down: bool, was_down: bool, now: Instant) -> Option<Hotkey> {
        if !self.bindings.double_tap.matches(usage) {
            if down {
                self.first_tap = None;
                self.armed = None;
            }
            return None;
        }
        if !down && self.armed.is_some_and(|(key, _)| key == usage) {
            self.armed = None;
        }
        self.arm_hold(usage, down, was_down, now);
        if self.bindings.trigger_mode == TriggerMode::Hold {
            return None;
        }
        if !down {
            self.tap_released = true;
            return None;
        }
        if was_down {
            return None;
        }
        let matched = self.tap_released
            && self.first_tap.is_some_and(|first| {
                now.checked_duration_since(first)
                    .is_some_and(|elapsed| elapsed <= DOUBLE_TAP_WINDOW)
            });
        if matched {
            self.armed = None;
        }
        self.first_tap = if matched { None } else { Some(now) };
        self.tap_released = false;
        matched.then_some(Hotkey::Home)
    }

    fn arm_hold(&mut self, usage: u32, down: bool, was_down: bool, now: Instant) {
        if !down || was_down || self.bindings.trigger_mode == TriggerMode::DoubleTap {
            return;
        }
        let other_held = self.pressed.iter().enumerate().any(|(index, held)| {
            *held && u32::try_from(index).is_ok_and(|key| KEYBOARD_PAGE | key != usage)
        });
        self.armed = if other_held {
            None
        } else {
            now.checked_add(self.bindings.hold)
                .map(|deadline| (usage, deadline))
        };
    }

    fn chord(&self, usage: u32) -> Option<Hotkey> {
        let mods = Mods((self.held_mods & 0x0f) | (self.held_mods >> 4));
        let chord = Chord { mods, key: usage };
        if self.bindings.home.contains(&chord) {
            return Some(Hotkey::Home);
        }
        if self.bindings.lock == chord {
            return Some(Hotkey::Lock);
        }
        chord.jump_target(self.bindings.jump_mods).map(Hotkey::Jump)
    }
}

#[cfg(test)]
mod tests;
