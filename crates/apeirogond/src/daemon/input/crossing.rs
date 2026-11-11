// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_core::input::{clamp_position, corner_push, edge_push};
use a7n_proto::v1::Landing;

use super::{Bounds, Daemon, Edge, MachineId, now};

impl Daemon {
    pub(super) async fn cross_edge(
        &mut self,
        bounds: Bounds,
        x: i32,
        y: i32,
        dx: i32,
        dy: i32,
        driven: bool,
    ) {
        let display = bounds;
        let bounds = (bounds.x, bounds.y, bounds.width, bounds.height);
        let Some((edge, overshoot, along)) = self.crossing_push(bounds, (x, y), (dx, dy), driven)
        else {
            self.input.poll_edge_pressure();
            return;
        };
        if !self.input.resistance.push(edge, overshoot, now()) {
            let progress = self.input.resistance.progress();
            self.input.sink.push(display, progress);
            return;
        }
        self.input.sink.clear();
        let Some((target, landing)) = self.crossing_exit(bounds, (x, y), (dx, dy), edge, along)
        else {
            if !driven && self.input.edge_no_target.replace(edge) != Some(edge) {
                crate::log::event!("kind=edge_no_target");
            }
            return;
        };
        self.input.edge_no_target = None;
        self.dispatch_crossing(target, landing, driven).await;
    }

    fn crossing_push(
        &mut self,
        bounds: (i32, i32, u32, u32),
        (x, y): (i32, i32),
        (dx, dy): (i32, i32),
        driven: bool,
    ) -> Option<(Edge, i32, f32)> {
        self.input.resistance.poll(now());
        if self.input.buttons.any_held() {
            self.input.fullscreen_hold.reset();
            self.input.resistance.push(Edge::Unspecified, 0, now());
            return None;
        }
        if self.input.paused || (!driven && self.input.focus.is_locked()) {
            self.input.fullscreen_hold.reset();
            return None;
        }
        let Some(push) = edge_push(bounds, x, y, dx, dy) else {
            self.input.fullscreen_hold.reset();
            if let Some((edge, _)) = self.input.resistance.progress() {
                let inward = match edge {
                    Edge::Left => dx.saturating_neg(),
                    Edge::Right => dx,
                    Edge::Top => dy.saturating_neg(),
                    Edge::Bottom => dy,
                    Edge::Unspecified => 0,
                }
                .min(0);
                self.input.resistance.push(edge, inward, now());
            }
            return None;
        };
        // ponytail: synchronous query on the async task; offload if measured latency grows.
        if self
            .input
            .fullscreen_hold
            .holds(true, || focused_fullscreen_at(bounds, x, y))
        {
            self.input.resistance.push(Edge::Unspecified, 0, now());
            return None;
        }
        Some(push)
    }

    fn crossing_exit(
        &self,
        bounds: (i32, i32, u32, u32),
        (x, y): (i32, i32),
        (dx, dy): (i32, i32),
        edge: Edge,
        along: f32,
    ) -> Option<(MachineId, Landing)> {
        let own = self.identity.machine_id;
        self.layout
            .state
            .exit(own, edge, along, |id| self.is_layout_target(id))
            .or_else(|| {
                let (edge, corner) = corner_push(bounds, x, y, dx, dy)?;
                self.layout
                    .state
                    .exit_corner(own, edge, corner, |id| self.is_layout_target(id))
            })
    }

    async fn dispatch_crossing(&mut self, target: MachineId, landing: Landing, driven: bool) {
        let actions = if driven {
            self.wol.pending = None;
            self.input.focus.handover(target, Some(landing))
        } else {
            if !self.is_linked(target) {
                if self
                    .auto_wake(target, crate::daemon::wol::WakeAction::Edge(landing), now())
                    .is_err()
                {
                    crate::log::event!("kind=wake_failed");
                }
                return;
            }
            self.wol.pending = None;
            self.take_peer(target, Some(landing), now())
        };
        self.input_actions(actions).await;
    }
}

fn focused_fullscreen_at(bounds: (i32, i32, u32, u32), x: i32, y: i32) -> bool {
    // Hook points can lie beyond the desktop; query the monitor at the clamped cursor.
    let (x, y) = clamp_position(bounds, x, y);
    let held = a7n_platform::fullscreen::focused_fullscreen_at(x, y);
    if held {
        crate::log::event!("kind=edge_held_fullscreen");
    }
    held
}

#[cfg(test)]
mod tests;
