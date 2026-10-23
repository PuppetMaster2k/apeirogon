// SPDX-License-Identifier: GPL-3.0-or-later

//! Local safety hotkeys. Time is supplied by the input owner.

use std::time::{Duration, Instant};

const KEYBOARD_PAGE: u32 = 0x0007_0000;
const MODIFIER_START: u32 = 0xe0;
const MODIFIER_COUNT: u32 = 8;
const KEY_COUNT: usize = 256;
const DIGIT_ONE: u32 = KEYBOARD_PAGE | 0x1e;
const DIGIT_NINE: u32 = KEYBOARD_PAGE | 0x26;
pub const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(400);

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

fn key_usage(token: &str) -> Option<u32> {
    let key = match token.as_bytes() {
        [letter @ b'a'..=b'z'] => u32::from(letter - b'a') + 0x04,
        [digit @ b'1'..=b'9'] => u32::from(digit - b'1') + 0x1e,
        [b'0'] => 0x27,
        _ => match token {
            "f1" => 0x3a,
            "f2" => 0x3b,
            "f3" => 0x3c,
            "f4" => 0x3d,
            "f5" => 0x3e,
            "f6" => 0x3f,
            "f7" => 0x40,
            "f8" => 0x41,
            "f9" => 0x42,
            "f10" => 0x43,
            "f11" => 0x44,
            "f12" => 0x45,
            "escape" => 0x29,
            "scroll_lock" => 0x47,
            "pause" => 0x48,
            "insert" => 0x49,
            "home" => 0x4a,
            "page_up" => 0x4b,
            "delete" => 0x4c,
            "end" => 0x4d,
            "page_down" => 0x4e,
            _ => return None,
        },
    };
    Some(KEYBOARD_PAGE | key)
}

/// Physical modifier sides that may trigger return home.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DoubleTap(u8);

impl DoubleTap {
    /// # Errors
    /// Rejects unknown sides or modifiers; `none` disables double tapping.
    pub fn parse(text: &str) -> Result<Self, HotkeyError> {
        let name = text.trim().to_ascii_lowercase();
        if name == "none" {
            return Ok(Self(0));
        }
        let invalid = || HotkeyError(format!("unknown double-tap key '{text}'"));
        let (side, modifier) = name.split_once('_').ok_or_else(invalid)?;
        let mods = Mods::token(modifier).ok_or_else(invalid)?;
        let mask = match side {
            "left" => mods.0,
            "right" => mods.0 << 4,
            "either" => mods.0 | (mods.0 << 4),
            _ => return Err(invalid()),
        };
        Ok(Self(mask))
    }
}

#[derive(Debug, Clone)]
pub struct Bindings {
    pub double_tap: DoubleTap,
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
            }
            return verdict;
        };
        let key = usage - KEYBOARD_PAGE;
        let was_down = std::mem::replace(&mut self.pressed[index], down);
        if (MODIFIER_START..MODIFIER_START + MODIFIER_COUNT).contains(&key) {
            let bit = 1 << (key - MODIFIER_START);
            if down {
                self.held_mods |= bit;
            } else {
                self.held_mods &= !bit;
            }
            verdict.action = self.tap(bit, down, was_down, now);
            return verdict;
        }
        if down {
            self.first_tap = None;
        }
        if self.consumed[index] {
            self.consumed[index] = down;
            verdict.forward = false;
        } else if down && !was_down {
            verdict.action = self.chord(usage);
            verdict.forward = verdict.action.is_none();
            self.consumed[index] = !verdict.forward;
        }
        verdict
    }

    fn tap(&mut self, bit: u8, down: bool, was_down: bool, now: Instant) -> Option<Hotkey> {
        if self.bindings.double_tap.0 & bit == 0 {
            if down {
                self.first_tap = None;
            }
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
        self.first_tap = if matched { None } else { Some(now) };
        self.tap_released = false;
        matched.then_some(Hotkey::Home)
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
