// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_proto::v1::{ButtonEvent, InputEvent, KeyEvent, input_event::Event};

/// Bounds memory and processing even when an owner sends arbitrary usages.
const HELD_INPUTS_MAX: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeldInput {
    Key(u32),
    Button(i32),
}

/// Keys and buttons still down, in press order. Applying input never allocates.
#[derive(Debug)]
pub struct HeldInputs {
    pressed: Vec<HeldInput>,
}

impl Default for HeldInputs {
    fn default() -> Self {
        Self::new()
    }
}

impl HeldInputs {
    #[must_use]
    pub fn new() -> Self {
        Self {
            pressed: Vec::with_capacity(HELD_INPUTS_MAX),
        }
    }

    /// Returns false when a new down exceeds the cap and must not be injected.
    pub fn apply(&mut self, event: &InputEvent) -> bool {
        let (input, down) = match &event.event {
            Some(Event::Key(key)) => (HeldInput::Key(key.usage), key.down),
            Some(Event::Button(button)) => (HeldInput::Button(button.button), button.down),
            _ => return true,
        };
        let position = self.pressed.iter().position(|held| *held == input);
        if down {
            if position.is_none() {
                if self.pressed.len() == HELD_INPUTS_MAX {
                    return false;
                }
                self.pressed.push(input);
            }
        } else if let Some(position) = position {
            self.pressed.remove(position);
        }
        true
    }

    /// Synthesises ups in press order and retains the input buffer for reuse.
    #[must_use]
    pub fn release_all(&mut self) -> Vec<InputEvent> {
        self.pressed
            .drain(..)
            .map(|input| InputEvent {
                event: Some(match input {
                    HeldInput::Key(usage) => Event::Key(KeyEvent {
                        usage,
                        down: false,
                        character: String::new(),
                    }),
                    HeldInput::Button(button) => Event::Button(ButtonEvent {
                        button,
                        down: false,
                    }),
                }),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use a7n_proto::v1::Button;

    use super::*;

    fn key(usage: u32, down: bool) -> InputEvent {
        InputEvent {
            event: Some(Event::Key(KeyEvent {
                usage,
                down,
                character: String::new(),
            })),
        }
    }

    fn button(down: bool) -> InputEvent {
        InputEvent {
            event: Some(Event::Button(ButtonEvent {
                button: Button::Left.into(),
                down,
            })),
        }
    }

    #[test]
    fn release_all_mixed_held_inputs_releases_in_press_order_and_empties() {
        // Arrange
        let mut held = HeldInputs::new();
        held.apply(&key(4, true));
        held.apply(&button(true));
        held.apply(&key(5, true));
        held.apply(&key(4, false));
        held.apply(&button(false));
        held.apply(&key(4, true));
        held.apply(&button(true));

        // Act
        let released = held.release_all();

        // Assert
        assert_eq!(released, vec![key(5, false), key(4, false), button(false)]);
        assert_eq!(held.release_all(), []);
    }

    #[test]
    fn apply_repeated_down_tracks_each_input_once() {
        // Arrange
        let mut held = HeldInputs::new();
        held.apply(&key(4, true));
        held.apply(&button(true));

        // Act
        held.apply(&key(4, true));
        held.apply(&button(true));

        // Assert
        assert_eq!(held.release_all(), vec![key(4, false), button(false)]);
    }

    #[test]
    fn apply_hostile_downs_caps_held_inputs_and_reuses_released_space() {
        // Arrange
        let mut held = HeldInputs::new();
        let cap = u32::try_from(HELD_INPUTS_MAX).expect("held input cap fits u32");
        for usage in 0..cap {
            held.apply(&key(usage, true));
        }

        // Act
        let overflow = held.apply(&key(cap, true));
        let repeated = held.apply(&key(1, true));
        let released = held.apply(&key(0, false));
        let reused = held.apply(&key(cap + 1, true));

        // Assert
        let expected: Vec<_> = (1..cap)
            .chain([cap + 1])
            .map(|usage| key(usage, false))
            .collect();
        assert!(!overflow);
        assert!(repeated);
        assert!(released);
        assert!(reused);
        assert_eq!(held.release_all(), expected);
    }
}
