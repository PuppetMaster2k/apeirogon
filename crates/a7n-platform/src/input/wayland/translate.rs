// SPDX-License-Identifier: GPL-3.0-or-later

//! Captured libei events -> daemon input.
//!
//! libei groups the events of one hardware report into a frame, so a wheel
//! that reports both smooth and discrete scrolling is seen once, not twice.

use a7n_proto::v1::{ButtonEvent, InputEvent, KeyEvent, ScrollEvent, ScrollUnit, input_event};

use super::buttons::button_from_evdev;
use crate::input::accumulate::MotionAccumulator;
use crate::input::evdev_keymap::hid_from_evdev;

/// libei's `ei_scroll.scroll_discrete` counts 120 per wheel notch.
pub(super) const DISCRETE_PER_NOTCH: f32 = 120.0;

/// What a libei receiver reports, reduced to what we use.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Raw {
    Motion { dx: f32, dy: f32 },
    Key { code: u32, down: bool },
    Button { code: u32, down: bool },
    ScrollSmooth { dx: f32, dy: f32 },
    ScrollDiscrete { dx: i32, dy: i32 },
    Frame,
}

#[derive(Debug, PartialEq)]
pub(super) enum Translated {
    /// Whole pixels the pointer moved.
    Motion {
        dx: i32,
        dy: i32,
    },
    Input(InputEvent),
}

#[derive(Debug, Default)]
pub(super) struct Translator {
    motion: MotionAccumulator,
    smooth: Option<(f32, f32)>,
    discrete: Option<(i32, i32)>,
}

impl Translator {
    pub(super) fn push(&mut self, raw: Raw) -> Option<Translated> {
        match raw {
            Raw::Motion { dx, dy } => {
                let (dx, dy) = self.motion.push(f64::from(dx), f64::from(dy));
                (dx != 0 || dy != 0).then_some(Translated::Motion { dx, dy })
            }
            Raw::Key { code, down } => {
                // An unmapped key has no protocol usage, so it is dropped.
                let usage = hid_from_evdev(u16::try_from(code).ok()?)?;
                Some(input(input_event::Event::Key(KeyEvent {
                    usage,
                    down,
                    character: String::new(),
                })))
            }
            Raw::Button { code, down } => {
                let button = button_from_evdev(code)?;
                Some(input(input_event::Event::Button(ButtonEvent {
                    button: button.into(),
                    down,
                })))
            }
            Raw::ScrollSmooth { dx, dy } => {
                let (x, y) = self.smooth.unwrap_or_default();
                self.smooth = Some((x + dx, y + dy));
                None
            }
            Raw::ScrollDiscrete { dx, dy } => {
                let (x, y) = self.discrete.unwrap_or_default();
                self.discrete = Some((x.saturating_add(dx), y.saturating_add(dy)));
                None
            }
            Raw::Frame => self.finish_scroll_frame(),
        }
    }

    /// Discrete wins: it is the wheel's own report, the smooth one is derived.
    #[allow(clippy::cast_precision_loss, reason = "wheel counts are small")]
    fn finish_scroll_frame(&mut self) -> Option<Translated> {
        let discrete = self.discrete.take();
        let smooth = self.smooth.take();
        let scroll = match (discrete, smooth) {
            (Some((dx, dy)), _) => ScrollEvent {
                dx: dx as f32 / DISCRETE_PER_NOTCH,
                dy: dy as f32 / DISCRETE_PER_NOTCH,
                unit: ScrollUnit::Line.into(),
            },
            (None, Some((dx, dy))) => ScrollEvent {
                dx,
                dy,
                unit: ScrollUnit::Pixel.into(),
            },
            (None, None) => return None,
        };
        Some(input(input_event::Event::Scroll(scroll)))
    }
}

fn input(event: input_event::Event) -> Translated {
    Translated::Input(InputEvent { event: Some(event) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scroll_of(translated: Option<&Translated>) -> Option<ScrollEvent> {
        match translated {
            Some(Translated::Input(InputEvent {
                event: Some(input_event::Event::Scroll(scroll)),
            })) => Some(*scroll),
            _ => None,
        }
    }

    #[test]
    fn push_frame_with_smooth_and_discrete_wheel_reports_lines_once() {
        // Arrange
        let mut translator = Translator::default();
        translator.push(Raw::ScrollSmooth { dx: 0.0, dy: 15.0 });
        translator.push(Raw::ScrollDiscrete { dx: 0, dy: 120 });

        // Act
        let scroll = scroll_of(translator.push(Raw::Frame).as_ref());

        // Assert
        let scroll = scroll.expect("one scroll event");
        assert_eq!(scroll.unit(), ScrollUnit::Line);
        assert!((scroll.dy - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn push_frame_with_smooth_only_reports_pixels() {
        // Arrange
        let mut translator = Translator::default();
        translator.push(Raw::ScrollSmooth { dx: 2.5, dy: -4.0 });

        // Act
        let scroll = scroll_of(translator.push(Raw::Frame).as_ref());

        // Assert
        let scroll = scroll.expect("one scroll event");
        assert_eq!(scroll.unit(), ScrollUnit::Pixel);
        assert!((scroll.dx - 2.5).abs() < f32::EPSILON);
        assert!((scroll.dy + 4.0).abs() < f32::EPSILON);
    }
}
