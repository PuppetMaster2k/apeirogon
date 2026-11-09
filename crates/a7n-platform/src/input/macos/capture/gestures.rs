// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_proto::v1::Gesture;
use objc2_app_kit::{NSEvent, NSTouchPhase};
use objc2_core_graphics::CGEvent;

#[path = "swipe.rs"]
mod swipe;

pub(super) const GESTURE_EVENT_TYPE: u32 = 29;
pub(super) use swipe::Swipes;

impl Swipes {
    pub(super) fn event(&mut self, event: &CGEvent) -> Option<Gesture> {
        let event = NSEvent::eventWithCGEvent(event)?;
        let touches = event.touchesMatchingPhase_inView(NSTouchPhase::Touching, None);
        let mut count = 0_u32;
        let (mut x, mut y) = (0.0, 0.0);
        for touch in &touches {
            if touch.isResting() {
                continue;
            }
            let point = touch.normalizedPosition();
            count += 1;
            x += point.x;
            y += point.y;
        }
        let centroid = if count == 0 {
            (0.0, 0.0)
        } else {
            (x / f64::from(count), y / f64::from(count))
        };
        self.update(count, centroid)
    }
}
