// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Instant;

use a7n_proto::MachineId;
use a7n_proto::limits;
use a7n_proto::v1::{Accept, Ack, Handover, Refuse, RefuseReason, envelope::Body};

use super::{Action, Cursor, Focus, Owner, Session, Take};

impl Focus {
    /// Start a direct take. A pending take ignores further takes; home always works.
    pub fn take(&mut self, take: Take, now: Instant) -> Vec<Action> {
        if take.target == self.own_id {
            return self.go_home();
        }
        if matches!(self.owner, Owner::Taking { .. }) || self.driven.is_some() {
            return Vec::new();
        }
        let Some(route) = self.next_route() else {
            return self.go_home();
        };
        let session = Session { take, route };
        let action = Self::send(
            session.take.target,
            Body::Focus(Self::focus_message(&session, session.take.target, false)),
        );
        let previous = match std::mem::replace(&mut self.owner, Owner::Home) {
            Owner::Driving(previous) => Some(previous),
            _ => None,
        };
        self.owner = Owner::Taking {
            session,
            previous,
            deadline: now + limits::TAKE_TIMEOUT,
        };
        vec![action]
    }

    pub fn on_accept(&mut self, peer: MachineId, accept: &Accept) -> Vec<Action> {
        if !self.pending_matches(peer, accept.route) {
            return Vec::new();
        }
        let Owner::Taking {
            session, previous, ..
        } = std::mem::replace(&mut self.owner, Owner::Home)
        else {
            return Vec::new();
        };
        let mut actions = Vec::new();
        if let Some(previous) = previous.filter(|p| p.take.target != peer) {
            self.let_go(previous.take.target, peer, &mut actions);
        }
        self.owner = Owner::Driving(session);
        actions.push(Action::Cursor(Cursor::Driving(peer)));
        actions
    }

    pub fn on_refuse(&mut self, peer: MachineId, refuse: &Refuse) -> Vec<Action> {
        if !self.pending_matches(peer, refuse.route) {
            return Vec::new();
        }
        match RefuseReason::try_from(refuse.reason) {
            // SENT_HOME is not produced in v1; defensively return home.
            Ok(RefuseReason::SentHome) => self.go_home(),
            _ => self.restore_previous(),
        }
    }

    fn restore_previous(&mut self) -> Vec<Action> {
        let Owner::Taking { previous, .. } = std::mem::replace(&mut self.owner, Owner::Home) else {
            return Vec::new();
        };
        let Some(mut previous) = previous else {
            return vec![Action::Cursor(Cursor::Home)];
        };
        let Some(route) = self.next_route() else {
            self.owner = Owner::Driving(previous);
            return self.go_home();
        };
        previous.route = route;
        let target = previous.take.target;
        let action = Self::send(
            target,
            Body::Focus(Self::focus_message(&previous, target, true)),
        );
        self.owner = Owner::Driving(previous);
        vec![action, Action::Cursor(Cursor::Driving(target))]
    }

    pub fn poll(&mut self, now: Instant) -> Vec<Action> {
        if self.next_deadline().is_some_and(|deadline| now >= deadline) {
            return self.go_home();
        }
        Vec::new()
    }

    /// Return home and cancel both halves of a pending transition.
    pub fn go_home(&mut self) -> Vec<Action> {
        let owner = std::mem::replace(&mut self.owner, Owner::Home);
        let mut actions = Vec::new();
        match owner {
            Owner::Home => return actions,
            Owner::Driving(session) => self.let_go(session.take.target, self.own_id, &mut actions),
            Owner::Taking {
                session, previous, ..
            } => {
                if let Some(previous) = previous.filter(|p| p.take.target != session.take.target) {
                    self.let_go(previous.take.target, self.own_id, &mut actions);
                }
                self.let_go(session.take.target, self.own_id, &mut actions);
            }
        }
        actions.push(Action::Cursor(Cursor::Home));
        actions
    }

    pub fn on_handover(
        &mut self,
        peer: MachineId,
        handover: &Handover,
        now: Instant,
    ) -> Vec<Action> {
        let Owner::Driving(session) = &self.owner else {
            return Vec::new();
        };
        if session.take.target != peer || session.route.0 != handover.route {
            return Vec::new();
        }
        let Ok(next) = MachineId::try_from(handover.next.as_slice()) else {
            return Vec::new();
        };
        if !handover.taken_over_by.is_empty() {
            if next != self.own_id
                || MachineId::try_from(handover.taken_over_by.as_slice()).is_err()
            {
                return Vec::new();
            }
            self.owner = Owner::Home;
            return vec![Action::Cursor(Cursor::Home)];
        }
        if self.is_locked() {
            return Vec::new();
        }
        if next == self.own_id {
            return self.go_home();
        }
        if !session.take.reach.contains(&next) {
            return Vec::new();
        }
        let take = Take {
            target: next,
            landing: handover.landing.clone(),
            ..session.take.clone()
        };
        self.take(take, now)
    }

    pub fn on_ack(&mut self, peer: MachineId, ack: &Ack) -> Vec<Action> {
        self.on_accepts(peer, !ack.locked)
    }

    /// A policy denial cancels both an active drive and a pending take.
    pub fn on_accepts(&mut self, peer: MachineId, accepts: bool) -> Vec<Action> {
        let is_target = match &self.owner {
            Owner::Home => false,
            Owner::Driving(session) => session.take.target == peer,
            Owner::Taking {
                session, previous, ..
            } => {
                session.take.target == peer
                    || previous.as_ref().is_some_and(|p| p.take.target == peer)
            }
        };
        if !accepts && is_target {
            self.go_home()
        } else {
            Vec::new()
        }
    }

    /// End ownership, pending takes and route history for this link.
    /// The daemon calls this whenever a link ends or is replaced by a newer
    /// link to the same peer.
    pub fn on_link_lost(&mut self, peer: MachineId) -> Vec<Action> {
        self.accepted.remove(&peer);
        let mut actions = Vec::new();
        if self
            .driven
            .as_ref()
            .is_some_and(|driven| driven.owner == peer)
        {
            self.end_driven(&mut actions);
        }
        let involved = match &self.owner {
            Owner::Home => false,
            Owner::Driving(session) => session.take.target == peer,
            Owner::Taking {
                session, previous, ..
            } => {
                session.take.target == peer
                    || previous.as_ref().is_some_and(|p| p.take.target == peer)
            }
        };
        if involved {
            actions.extend(self.go_home());
            actions.retain(|action| !matches!(action, Action::Send { peer: destination, .. } if *destination == peer));
        }
        actions
    }

    fn pending_matches(&self, peer: MachineId, route: u64) -> bool {
        matches!(&self.owner, Owner::Taking { session, .. } if session.take.target == peer && session.route.0 == route)
    }

    fn let_go(&mut self, peer: MachineId, target: MachineId, actions: &mut Vec<Action>) {
        if let Some(route) = self.next_route() {
            actions.push(Self::send(
                peer,
                Body::Focus(a7n_proto::v1::Focus {
                    route: route.0,
                    target: target.as_bytes().to_vec(),
                    ..Default::default()
                }),
            ));
        }
    }
}
