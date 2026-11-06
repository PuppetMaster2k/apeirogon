// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn on_motion_four_big_reversals_fires_once() {
    // Arrange
    let mut detector = ShakeDetector::new(4);
    let start = Instant::now();
    let motion = [20, 20, 0, -40, 40, -40, 40, 0, 40];

    // Act
    let fired: Vec<_> = motion
        .into_iter()
        .zip(0..)
        .map(|(dx, index)| detector.on_motion(dx, start + Duration::from_millis(index * 50)))
        .collect();

    // Assert
    assert_eq!(
        fired,
        [false, false, false, false, false, false, true, false, false]
    );
}

#[test]
fn on_motion_reversals_spread_over_window_does_not_fire() {
    // Arrange
    let mut detector = ShakeDetector::new(4);
    let start = Instant::now();

    // Act
    let fired: Vec<_> = [40, -40, 40, -40, 40, -40, 40, -40]
        .into_iter()
        .zip(0..)
        .map(|(dx, index)| detector.on_motion(dx, start + Duration::from_millis(index * 250)))
        .collect();

    // Assert
    assert!(!fired.into_iter().any(|fired| fired));
}

#[test]
fn on_motion_small_jitter_does_not_count() {
    // Arrange
    let mut detector = ShakeDetector::new(4);
    let now = Instant::now();

    // Act
    let fired: Vec<_> = [10, 10, 10, -10, -10, -10]
        .repeat(10)
        .into_iter()
        .map(|dx| detector.on_motion(dx, now))
        .collect();

    // Assert
    assert!(!fired.into_iter().any(|fired| fired));
}

#[test]
fn on_motion_disabled_never_fires() {
    // Arrange
    let mut detector = ShakeDetector::new(0);
    let now = Instant::now();

    // Act
    let fired: Vec<_> = [i32::MAX, i32::MIN]
        .repeat(10)
        .into_iter()
        .map(|dx| detector.on_motion(dx, now))
        .collect();

    // Assert
    assert!(!fired.into_iter().any(|fired| fired));
}

#[test]
fn on_motion_custom_count_needs_six_reversals() {
    // Arrange
    let mut detector = ShakeDetector::new(6);
    let now = Instant::now();

    // Act
    let fired: Vec<_> = [40, -40, 40, -40, 40, -40, 40]
        .into_iter()
        .map(|dx| detector.on_motion(dx, now))
        .collect();

    // Assert
    assert_eq!(fired, [false, false, false, false, false, false, true]);
}
