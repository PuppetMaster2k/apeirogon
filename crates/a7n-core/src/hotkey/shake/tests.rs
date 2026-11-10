// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn on_motion_four_big_reversals_fires_once() {
    // Arrange
    let mut detector = ShakeDetector::new(4);
    let start = Instant::now();
    let motion = [40, 40, 0, -80, 80, -80, 80, 0, 80];

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
    let fired: Vec<_> = [80, -80, 80, -80, 80, -80, 80, -80]
        .into_iter()
        .zip(0..)
        .map(|(dx, index)| detector.on_motion(dx, start + Duration::from_millis(index * 250)))
        .collect();

    // Assert
    assert!(!fired.into_iter().any(|fired| fired));
}

#[test]
fn on_motion_reversals_below_min_travel_does_not_fire() {
    // Arrange
    let mut detector = ShakeDetector::new(SHAKE_DEFAULT);
    let now = Instant::now();

    // Act
    let fired = [40, -40]
        .repeat(10)
        .into_iter()
        .any(|dx| detector.on_motion(dx, now));

    // Assert
    assert!(!fired);
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
fn on_motion_default_count_needs_six_reversals() {
    // Arrange
    let mut detector = ShakeDetector::new(SHAKE_DEFAULT);
    let now = Instant::now();

    // Act
    let fired: Vec<_> = [80, -80, 80, -80, 80, -80, 80]
        .into_iter()
        .map(|dx| detector.on_motion(dx, now))
        .collect();

    // Assert
    assert_eq!(fired, [false, false, false, false, false, false, true]);
}
