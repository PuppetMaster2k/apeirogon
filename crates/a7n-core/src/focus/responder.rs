// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Instant;

use a7n_proto::limits;
use a7n_proto::v1::{
    Accept, Focus as WireFocus, Handover, InputEvent, Landing, Refuse, RefuseReason, envelope::Body,
};
use a7n_proto::{MachineId, Route};

use super::{Action, Driven, Focus};

impl Focus {
    /// Process a Focus from an authenticated peer. Policy is supplied per message.
    pub fn on_focus(
        &mut self,
        peer: MachineId,
        focus: &WireFocus,
        allow_drive: bool,
        now: Instant,
    ) -> Vec<Action> {
        if self
            .accepted
            .get(&peer)
            .is_some_and(|route| focus.route <= route.0)
        {
            return Vec::new();
        }
        let Ok(target) = MachineId::try_from(focus.target.as_slice()) else {
            return vec![Self::refuse(
                peer,
                focus.route,
                RefuseReason::Malformed,
                None,
            )];
        };
        let Ok(reach) = focus
            .reach
            .iter()
            .map(|id| MachineId::try_from(id.as_slice()))
            .collect::<Result<Vec<_>, _>>()
        else {
            return vec![Self::refuse(
                peer,
                focus.route,
                RefuseReason::Malformed,
                None,
            )];
        };
        if target != self.own_id {
            // Remember even a let-go arriving before its take, so delayed takes stay stale.
            self.accepted.insert(peer, Route(focus.route));
            let mut actions = Vec::new();
            if self
                .driven
                .as_ref()
                .is_some_and(|driven| driven.owner == peer)
            {
                self.end_driven(&mut actions);
            }
            return actions;
        }
        if !allow_drive {
            return vec![Self::refuse(
                peer,
                focus.route,
                RefuseReason::NotAllowed,
                None,
            )];
        }
        self.accept_take(peer, focus, reach, now)
    }

    fn accept_take(
        &mut self,
        peer: MachineId,
        focus: &WireFocus,
        reach: Vec<MachineId>,
        now: Instant,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        if let Some(driven) = &self.driven {
            if driven.owner != peer {
                if now.saturating_duration_since(driven.last_input) < limits::IDLE_TAKEOVER {
                    return vec![Self::refuse(
                        peer,
                        focus.route,
                        RefuseReason::Owned,
                        Some(driven.owner),
                    )];
                }
                self.take_over(peer, &mut actions);
            }
        } else if self
            .last_local_input
            .is_some_and(|last| now.saturating_duration_since(last) < limits::IDLE_TAKEOVER)
        {
            return vec![Self::refuse(
                peer,
                focus.route,
                RefuseReason::Busy,
                Some(self.own_id),
            )];
        }
        actions.extend(self.go_home());
        self.accepted.insert(peer, Route(focus.route));
        self.driven = Some(Driven {
            owner: peer,
            route: Route(focus.route),
            last_input: now,
            reach,
        });
        actions.push(Self::send(
            peer,
            Body::Accept(Accept { route: focus.route }),
        ));
        actions.push(Action::Driven {
            owner: peer,
            landing: if focus.stay {
                None
            } else {
                focus.landing.clone()
            },
            resistance_px: focus.resistance_px,
        });
        actions
    }

    /// Call for an accepted `InputBatch` or `Motion`, before injecting input.
    /// Rejected routes never update the idle timer. This path does not allocate.
    pub fn on_remote_input(&mut self, peer: MachineId, route: Route, now: Instant) -> bool {
        let Some(driven) = self.driven.as_mut() else {
            return false;
        };
        if driven.owner != peer || driven.route != route {
            return false;
        }
        driven.last_input = now;
        true
    }

    /// Whether to inject a reliable event after route and held-input checks.
    /// Excess downs beyond the held-input cap are dropped before injection.
    pub fn on_input_event(
        &mut self,
        peer: MachineId,
        route: Route,
        event: &InputEvent,
        now: Instant,
    ) -> bool {
        if !self.on_remote_input(peer, route, now) {
            return false;
        }
        self.held.apply(event)
    }

    /// Whether to apply physical input, followed by effects of a local takeover.
    /// An active remote owner suppresses local input without refreshing local activity.
    pub fn on_local_input(&mut self, now: Instant) -> (bool, Vec<Action>) {
        let mut actions = Vec::new();
        if let Some(driven) = &self.driven {
            if now.saturating_duration_since(driven.last_input) < limits::IDLE_TAKEOVER {
                return (false, actions);
            }
            self.take_over(self.own_id, &mut actions);
        }
        self.last_local_input = Some(now);
        (true, actions)
    }

    /// Request a reachable peer (or the owner's home); ownership ends on let-go.
    #[must_use]
    pub fn handover(&self, next: MachineId, landing: Option<Landing>) -> Vec<Action> {
        let Some(driven) = &self.driven else {
            return Vec::new();
        };
        if next != driven.owner && !driven.reach.contains(&next) {
            return Vec::new();
        }
        vec![Self::send(
            driven.owner,
            Body::Handover(Handover {
                route: driven.route.0,
                next: next.as_bytes().to_vec(),
                landing,
                taken_over_by: Vec::new(),
            }),
        )]
    }

    /// End this peer's ownership immediately when local policy is revoked.
    pub fn deny_drive(&mut self, peer: MachineId) -> Vec<Action> {
        let mut actions = Vec::new();
        if self
            .driven
            .as_ref()
            .is_some_and(|driven| driven.owner == peer)
        {
            self.end_driven(&mut actions);
        }
        actions
    }

    pub(super) fn end_driven(&mut self, actions: &mut Vec<Action>) {
        self.driven = None;
        let released = self.held.release_all();
        if !released.is_empty() {
            actions.push(Action::Release(released));
        }
    }

    fn take_over(&mut self, replacement: MachineId, actions: &mut Vec<Action>) {
        let Some(driven) = self.driven.take() else {
            return;
        };
        self.end_driven(actions);
        actions.push(Self::send(
            driven.owner,
            Body::Handover(Handover {
                route: driven.route.0,
                next: driven.owner.as_bytes().to_vec(),
                landing: None,
                taken_over_by: replacement.as_bytes().to_vec(),
            }),
        ));
    }

    fn refuse(
        peer: MachineId,
        route: u64,
        reason: RefuseReason,
        other: Option<MachineId>,
    ) -> Action {
        Self::send(
            peer,
            Body::Refuse(Refuse {
                route,
                reason: reason.into(),
                other: other.map_or_else(Vec::new, |id| id.as_bytes().to_vec()),
            }),
        )
    }
}
