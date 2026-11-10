// SPDX-License-Identifier: GPL-3.0-or-later

//! Receiver-local swipe shortcuts, independent of sender modifier remapping.

use a7n_proto::v1::{Gesture, KeyEvent};

use crate::hotkey::Chord;

#[derive(Debug, Clone, Default)]
pub struct Bindings {
    pub swipe_up: Option<Chord>,
    pub swipe_down: Option<Chord>,
    pub swipe_left: Option<Chord>,
    pub swipe_right: Option<Chord>,
}

impl Bindings {
    pub fn events(&self, gesture: Gesture) -> impl Iterator<Item = KeyEvent> {
        let chord = match gesture {
            Gesture::SwipeUp => self.swipe_up,
            Gesture::SwipeDown => self.swipe_down,
            Gesture::SwipeLeft => self.swipe_left,
            Gesture::SwipeRight => self.swipe_right,
            // Pinch and spread have no keyboard-shortcut equivalent.
            _ => None,
        };
        chord.into_iter().flat_map(|chord| {
            chord
                .mods
                .usages()
                .map(|usage| (usage, true))
                .chain([(chord.key, true), (chord.key, false)])
                .chain(chord.mods.usages().rev().map(|usage| (usage, false)))
                .map(|(usage, down)| KeyEvent {
                    usage,
                    down,
                    character: String::new(),
                })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_default_left_chord_presses_and_releases_in_order() {
        // Arrange
        let bindings = Bindings {
            swipe_left: Some(Chord::parse("ctrl+meta+left").unwrap()),
            ..Bindings::default()
        };

        // Act
        let events: Vec<_> = bindings
            .events(Gesture::SwipeLeft)
            .map(|key| (key.usage, key.down))
            .collect();

        // Assert
        assert_eq!(
            events,
            [
                (0x0007_00e0, true),
                (0x0007_00e3, true),
                (0x0007_0050, true),
                (0x0007_0050, false),
                (0x0007_00e3, false),
                (0x0007_00e0, false)
            ]
        );
    }
}
