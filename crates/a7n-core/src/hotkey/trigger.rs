// SPDX-License-Identifier: GPL-3.0-or-later

use super::keys::{key_usage, trigger_name};
use super::{HotkeyError, Mods, modifier_bit};

pub const HOLD_DEFAULT: u32 = 800;
pub const HOLD_MIN: u32 = 300;
pub const HOLD_MAX: u32 = 3000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TriggerMode {
    #[default]
    DoubleTap,
    Hold,
    Both,
}

impl TriggerMode {
    /// # Errors
    /// Rejects modes other than `double_tap`, `hold` and `both`.
    pub fn parse(text: &str) -> Result<Self, HotkeyError> {
        match text {
            "double_tap" => Ok(Self::DoubleTap),
            "hold" => Ok(Self::Hold),
            "both" => Ok(Self::Both),
            _ => Err(HotkeyError(format!("unknown trigger mode '{text}'"))),
        }
    }
}

/// Trigger selected by the backwards-compatible `double_tap` config key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DoubleTap(TriggerKey);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TriggerKey {
    None,
    Modifiers(u8),
    Key(u32),
}

impl DoubleTap {
    /// # Errors
    /// Rejects keys that type text or are unsuitable as safety triggers.
    pub fn parse(text: &str) -> Result<Self, HotkeyError> {
        let name = text.trim().to_ascii_lowercase();
        if name == "none" {
            return Ok(Self(TriggerKey::None));
        }
        if let Some(key) = key_usage(&name).filter(|key| trigger_name(*key).is_some()) {
            return Ok(Self(TriggerKey::Key(key)));
        }
        if let Some(modifier) = name.strip_prefix("either_").and_then(Mods::token) {
            return Ok(Self(TriggerKey::Modifiers(modifier.0 | (modifier.0 << 4))));
        }
        Err(HotkeyError(format!("unknown trigger key '{text}'")))
    }

    #[must_use]
    pub fn matches(self, usage: u32) -> bool {
        match self.0 {
            TriggerKey::None => false,
            TriggerKey::Key(key) => key == usage,
            TriggerKey::Modifiers(mask) => modifier_bit(usage).is_some_and(|bit| mask & bit != 0),
        }
    }

    #[must_use]
    pub fn collides(self, chord: super::Chord) -> bool {
        chord.mods == Mods::default() && self.matches(chord.key)
    }
}
