// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn on_motion_reversal_count_size_and_window_decide_whether_it_fires() {
    // Arrange: (count, motion, step in ms, expected to fire)
    let cases: [(u8, &[i32], u64, bool); 5] = [
        (4, &[40, 40, 0, -80, 80, -80, 80, 0, 80], 50, true),
        (4, &[80, -80, 80, -80, 80, -80, 80, -80], 250, false),
        (
            SHAKE_DEFAULT,
            &[40, -40, 40, -40, 40, -40, 40, -40],
            0,
            false,
        ),
        (SHAKE_DEFAULT, &[80, -80, 80, -80, 80, -80, 80], 0, true),
        (0, &[i32::MAX, i32::MIN, i32::MAX, i32::MIN], 0, false),
    ];
    for (count, motion, step_ms, expected) in cases {
        let mut detector = ShakeDetector::new(count);
        let start = Instant::now();

        // Act
        let fired = motion.iter().zip(0..).any(|(&dx, index)| {
            detector.on_motion(dx, start + Duration::from_millis(index * step_ms))
        });

        // Assert
        assert_eq!(fired, expected, "count {count}, step {step_ms} ms");
    }
}
