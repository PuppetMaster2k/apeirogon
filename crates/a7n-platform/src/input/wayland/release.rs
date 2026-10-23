// SPDX-License-Identifier: GPL-3.0-or-later

//! When to give captured input back to the compositor.
//!
//! The compositor, not us, decides when capture starts (a pointer pushed
//! through a barrier). Everything after that is ours to end, and a capture we
//! do not end locks the local user out. This state machine is pure: time comes
//! in as a parameter, the answer goes out as a [`ReleaseOrder`].

use std::time::{Duration, Instant};

use super::zones;
use crate::input::{Bounds, CaptureControl, DeadMan};

/// How long a push may stall without the daemon answering it with
/// `Grab(true)`. The window restarts whenever the push gets further out, so a
/// slow push through edge resistance is not cut short; a push that stops
/// leading anywhere must not hold the cursor.
pub(super) const TAKE_WINDOW: Duration = Duration::from_secs(1);

/// What to pass to the portal's `Release`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ReleaseOrder {
    pub(super) activation_id: u32,
    /// Where the compositor should put the cursor back. `None` once the
    /// crossing happened: the daemon then lands the cursor itself.
    pub(super) cursor: Option<(f64, f64)>,
}

#[derive(Debug)]
struct Activation {
    id: u32,
    /// Where the push started, moved inside the zones.
    origin: (i32, i32),
    /// Virtual pointer position: it keeps moving past the edge, which is how
    /// the daemon sees a push.
    position: (i32, i32),
    /// Last time the push got further from `origin`.
    since: Instant,
    /// Largest distance from `origin` reached so far.
    reach: i32,
    /// The daemon asked for the grab, so this capture is a crossing.
    crossed: bool,
    /// The position has left the zones at least once. The portal may report
    /// the activation point on the last pixel, which must not count as
    /// "moved back".
    left_zones: bool,
}

#[derive(Debug)]
pub(super) struct CaptureFlow {
    zones: Vec<Bounds>,
    deadman: DeadMan,
    grab_wanted: bool,
    active: Option<Activation>,
}

impl CaptureFlow {
    pub(super) fn new(zones: Vec<Bounds>, now: Instant) -> Self {
        Self {
            zones,
            deadman: DeadMan::new(now),
            grab_wanted: false,
            active: None,
        }
    }

    pub(super) fn set_zones(&mut self, zones: Vec<Bounds>) {
        self.zones = zones;
    }

    pub(super) fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// The compositor started capturing at `cursor`, which is usually just
    /// outside the zones.
    pub(super) fn activate(&mut self, id: u32, cursor: (f64, f64), now: Instant) {
        let position = (round_to_i32(cursor.0), round_to_i32(cursor.1));
        let origin = zones::nearest_inside(&self.zones, position.0, position.1).unwrap_or(position);
        self.active = Some(Activation {
            id,
            origin,
            position,
            since: now,
            reach: distance(origin, position),
            // A daemon that already wants the grab (it is being driven) keeps
            // every capture until it lets go.
            crossed: self.grab_wanted,
            left_zones: !zones::contains(&self.zones, position.0, position.1),
        });
    }

    /// The compositor ended the capture by itself.
    pub(super) fn deactivate(&mut self, id: u32) {
        if self.active.as_ref().is_some_and(|active| active.id == id) {
            self.active = None;
        }
    }

    /// Moves the virtual pointer and returns where it is now, or `None` when
    /// nothing is captured.
    pub(super) fn motion(&mut self, dx: i32, dy: i32, now: Instant) -> Option<(i32, i32)> {
        let active = self.active.as_mut()?;
        active.position = (
            active.position.0.saturating_add(dx),
            active.position.1.saturating_add(dy),
        );
        let reach = distance(active.origin, active.position);
        if reach > active.reach {
            active.reach = reach;
            active.since = now;
        }
        let inside = zones::contains(&self.zones, active.position.0, active.position.1);
        active.left_zones |= !inside;
        Some(active.position)
    }

    pub(super) fn control(
        &mut self,
        command: CaptureControl,
        now: Instant,
    ) -> Option<ReleaseOrder> {
        match command {
            CaptureControl::Keepalive => self.deadman.keepalive(now),
            CaptureControl::Grab(true) => {
                // A grab request is itself proof that the daemon is alive.
                self.deadman.keepalive(now);
                self.grab_wanted = true;
                if let Some(active) = &mut self.active {
                    active.crossed = true;
                }
            }
            CaptureControl::Grab(false) => {
                self.grab_wanted = false;
                return self.release();
            }
        }
        None
    }

    /// Checks the conditions that end a capture without a command: no take
    /// within [`TAKE_WINDOW`], the pointer back inside the zones before a take,
    /// and a silent daemon during a take.
    pub(super) fn poll(&mut self, now: Instant) -> Option<ReleaseOrder> {
        let active = self.active.as_ref()?;
        let done = if active.crossed {
            self.deadman.expired(now)
        } else {
            let waited = now.saturating_duration_since(active.since) >= TAKE_WINDOW;
            let moved_back = active.left_zones
                && zones::contains(&self.zones, active.position.0, active.position.1);
            waited || moved_back
        };
        if done { self.release() } else { None }
    }

    /// Ends whatever capture is running, for shutdown.
    pub(super) fn release(&mut self) -> Option<ReleaseOrder> {
        let active = self.active.take()?;
        let cursor =
            (!active.crossed).then(|| (f64::from(active.origin.0), f64::from(active.origin.1)));
        Some(ReleaseOrder {
            activation_id: active.id,
            cursor,
        })
    }
}

/// Chebyshev distance: a push is as far out as its longest axis.
fn distance(from: (i32, i32), to: (i32, i32)) -> i32 {
    from.0
        .abs_diff(to.0)
        .max(from.1.abs_diff(to.1))
        .try_into()
        .unwrap_or(i32::MAX)
}

#[allow(clippy::cast_possible_truncation, reason = "clamped to the i32 range")]
fn round_to_i32(value: f64) -> i32 {
    if value.is_nan() {
        return 0;
    }
    value
        .round()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{DEADMAN_TIMEOUT, KEEPALIVE_INTERVAL};

    const ACTIVATION: u32 = 7;

    fn zones() -> Vec<Bounds> {
        vec![Bounds {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }]
    }

    /// A flow with a push through the right edge, activated at `now`.
    fn pushed_flow(now: Instant) -> CaptureFlow {
        let mut flow = CaptureFlow::new(zones(), now);
        flow.activate(ACTIVATION, (1925.0, 500.0), now);
        flow
    }

    #[test]
    fn control_grab_off_before_take_releases_at_activation_point() {
        // Arrange
        let now = Instant::now();
        let mut flow = pushed_flow(now);

        // Act
        let order = flow.control(CaptureControl::Grab(false), now);

        // Assert
        assert_eq!(
            order,
            Some(ReleaseOrder {
                activation_id: ACTIVATION,
                cursor: Some((1919.0, 500.0))
            })
        );
        assert!(!flow.is_active());
    }

    #[test]
    fn control_grab_off_after_take_releases_without_cursor() {
        // Arrange
        let now = Instant::now();
        let mut flow = pushed_flow(now);
        flow.control(CaptureControl::Grab(true), now);

        // Act
        let order = flow.control(CaptureControl::Grab(false), now);

        // Assert
        assert_eq!(
            order,
            Some(ReleaseOrder {
                activation_id: ACTIVATION,
                cursor: None
            })
        );
    }

    #[test]
    fn poll_no_take_within_window_releases_at_activation_point() {
        // Arrange
        let now = Instant::now();
        let mut flow = pushed_flow(now);

        // Act
        let early = flow.poll(now + TAKE_WINDOW.saturating_sub(Duration::from_millis(1)));
        let late = flow.poll(now + TAKE_WINDOW);

        // Assert
        assert_eq!(early, None);
        assert_eq!(
            late,
            Some(ReleaseOrder {
                activation_id: ACTIVATION,
                cursor: Some((1919.0, 500.0))
            })
        );
    }

    #[test]
    fn poll_push_still_getting_further_keeps_capturing_past_the_window() {
        // Arrange
        let now = Instant::now();
        let mut flow = pushed_flow(now);
        let later = now + TAKE_WINDOW;
        flow.motion(40, 0, later);

        // Act
        let order = flow.poll(later + TAKE_WINDOW.saturating_sub(Duration::from_millis(1)));

        // Assert
        assert_eq!(order, None);
    }

    #[test]
    fn poll_pointer_moved_back_inside_before_take_releases() {
        // Arrange
        let now = Instant::now();
        let mut flow = pushed_flow(now);
        flow.motion(10, 0, now);

        // Act
        let still_out = flow.poll(now);
        flow.motion(-40, 0, now);
        let back = flow.poll(now);

        // Assert
        assert_eq!(still_out, None);
        assert_eq!(
            back,
            Some(ReleaseOrder {
                activation_id: ACTIVATION,
                cursor: Some((1919.0, 500.0))
            })
        );
    }

    #[test]
    fn poll_activation_on_last_pixel_does_not_count_as_moved_back() {
        // Arrange
        let now = Instant::now();
        let mut flow = CaptureFlow::new(zones(), now);
        flow.activate(ACTIVATION, (1919.0, 500.0), now);

        // Act
        flow.motion(0, 3, now);
        let order = flow.poll(now);

        // Assert
        assert_eq!(order, None);
    }

    #[test]
    fn poll_silent_daemon_during_take_releases_at_deadman_timeout() {
        // Arrange
        let now = Instant::now();
        let mut flow = pushed_flow(now);
        flow.control(CaptureControl::Grab(true), now);
        flow.control(CaptureControl::Keepalive, now + KEEPALIVE_INTERVAL);

        // Act
        let alive = flow.poll(
            now + KEEPALIVE_INTERVAL + DEADMAN_TIMEOUT.saturating_sub(Duration::from_millis(1)),
        );
        let dead = flow.poll(now + KEEPALIVE_INTERVAL + DEADMAN_TIMEOUT);

        // Assert
        assert_eq!(alive, None);
        assert_eq!(
            dead,
            Some(ReleaseOrder {
                activation_id: ACTIVATION,
                cursor: None
            })
        );
    }

    #[test]
    fn poll_take_within_window_keeps_capturing_past_the_window() {
        // Arrange
        let now = Instant::now();
        let mut flow = pushed_flow(now);
        let later = now + TAKE_WINDOW * 3;

        // Act
        flow.control(CaptureControl::Grab(true), now + Duration::from_millis(300));
        flow.control(CaptureControl::Keepalive, later);
        flow.motion(-100, 0, now);
        let order = flow.poll(later);

        // Assert
        assert_eq!(order, None);
        assert!(flow.is_active());
    }

    #[test]
    fn activate_grab_already_wanted_is_a_crossing_from_the_start() {
        // Arrange
        let now = Instant::now();
        let mut flow = CaptureFlow::new(zones(), now);
        flow.control(CaptureControl::Grab(true), now);

        // Act
        flow.activate(ACTIVATION, (1925.0, 500.0), now);
        let after_window = flow.poll(now + TAKE_WINDOW);

        // Assert
        assert_eq!(after_window, None);
    }

    #[test]
    fn motion_without_activation_is_ignored() {
        // Arrange
        let now = Instant::now();
        let mut flow = CaptureFlow::new(zones(), now);

        // Act
        let position = flow.motion(5, 5, now);

        // Assert
        assert_eq!(position, None);
    }
}
