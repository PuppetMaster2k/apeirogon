// SPDX-License-Identifier: GPL-3.0-or-later

use std::ops::RangeInclusive;

use a7n_proto::v1::Gesture;

// Normalized touch coordinates make the threshold independent of trackpad size.
const MIN_TRAVEL: f64 = 0.12;
const SWIPE_FINGERS: RangeInclusive<u32> = 3..=4;

#[derive(Default)]
pub(crate) struct Swipes {
    origin: Option<(f64, f64)>,
    fingers: u32,
    emitted: bool,
}

impl Swipes {
    pub(crate) fn update(&mut self, fingers: u32, point: (f64, f64)) -> Option<Gesture> {
        if fingers == 0 {
            *self = Self::default();
            return None;
        }
        let swipe_fingers = SWIPE_FINGERS.contains(&fingers);
        if !swipe_fingers || self.fingers != fingers {
            self.origin = None;
        }
        self.fingers = fingers;
        if !swipe_fingers || self.emitted {
            return None;
        }
        let origin = *self.origin.get_or_insert(point);
        let (dx, dy) = (point.0 - origin.0, point.1 - origin.1);
        if dx.abs().max(dy.abs()) < MIN_TRAVEL {
            return None;
        }
        self.emitted = true;
        Some(if dx.abs() > dy.abs() {
            if dx > 0.0 {
                Gesture::SwipeRight
            } else {
                Gesture::SwipeLeft
            }
        } else if dy > 0.0 {
            Gesture::SwipeUp
        } else {
            Gesture::SwipeDown
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_dominant_axis_emits_direction() {
        // Arrange
        for fingers in [3, 4] {
            for (point, expected) in [
                ((0.8, 0.6), Gesture::SwipeRight),
                ((0.2, 0.4), Gesture::SwipeLeft),
                ((0.6, 0.8), Gesture::SwipeUp),
                ((0.4, 0.2), Gesture::SwipeDown),
            ] {
                let mut swipes = Swipes::default();
                swipes.update(fingers, (0.5, 0.5));

                // Act
                let gesture = swipes.update(fingers, point);

                // Assert
                assert_eq!(gesture, Some(expected));
            }
        }
    }

    #[test]
    fn update_emitted_swipe_reversal_or_changed_count_emits_nothing() {
        // Arrange
        let mut reversed = Swipes::default();
        reversed.update(3, (0.5, 0.5));
        reversed.update(3, (0.8, 0.5));
        let mut recounted = Swipes::default();
        recounted.update(3, (0.5, 0.5));
        recounted.update(3, (0.8, 0.5));
        recounted.update(4, (0.2, 0.5));

        // Act
        let repeated = reversed.update(3, (0.2, 0.5));
        let changed_count = recounted.update(4, (0.8, 0.5));

        // Assert
        assert_eq!((repeated, changed_count), (None, None));
    }

    #[test]
    fn update_fingers_lifted_allows_next_swipe() {
        // Arrange
        let mut swipes = Swipes::default();
        swipes.update(3, (0.5, 0.5));
        swipes.update(3, (0.8, 0.5));
        swipes.update(0, (0.0, 0.0));
        swipes.update(4, (0.5, 0.5));

        // Act
        let next = swipes.update(4, (0.2, 0.5));

        // Assert
        assert_eq!(next, Some(Gesture::SwipeLeft));
    }
}
