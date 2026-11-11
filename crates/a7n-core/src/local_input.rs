// SPDX-License-Identifier: GPL-3.0-or-later

//! Physical inputs reserved for the local machine.

use a7n_proto::v1::{Button, input_event::Event};

use crate::hotkey::{key_name, key_usage};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LocalInput {
    Key(u32),
    Button(i32),
}

impl LocalInput {
    /// # Errors
    /// Rejects unknown names and primary mouse buttons.
    pub fn parse(name: &str) -> Result<Self, &'static str> {
        match name {
            "left" | "right" => Err("left/right mouse buttons cannot stay local"),
            "middle" => Ok(Self::Button(Button::Middle.into())),
            "back" => Ok(Self::Button(Button::Back.into())),
            "forward" => Ok(Self::Button(Button::Forward.into())),
            _ => key_usage(name)
                .map(Self::Key)
                .ok_or("unknown key or button name"),
        }
    }

    #[must_use]
    pub fn from_event(event: &Event) -> Option<(Self, bool)> {
        match event {
            Event::Key(key) => Some((Self::Key(key.usage), key.down)),
            Event::Button(button) => Some((Self::Button(button.button), button.down)),
            _ => None,
        }
    }

    #[must_use]
    pub fn name(self) -> Option<String> {
        match self {
            Self::Key(usage) => key_name(usage).filter(|name| Self::parse(name) == Ok(self)),
            Self::Button(button) => match Button::try_from(button).ok()? {
                Button::Middle => Some("middle".into()),
                Button::Back => Some("back".into()),
                Button::Forward => Some("forward".into()),
                _ => None,
            },
        }
    }
}
