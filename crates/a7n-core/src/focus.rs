// SPDX-License-Identifier: GPL-3.0-or-later

//! Focus ownership and routing (PROTOCOL.md 8). The caller supplies time,
//! authenticated peer identities and drive policy, and executes returned actions.
//! Poll due deadlines before processing incoming control messages.

use std::collections::BTreeMap;
use std::time::Instant;

use a7n_proto::v1::{Focus as WireFocus, InputEvent, Landing, envelope::Body};
use a7n_proto::{MachineId, Route};

pub mod edge;
pub mod held;
mod owner;
mod responder;

use held::HeldInputs;

/// A direct take, with the owner's current reachable peers.
#[derive(Debug, Clone, PartialEq)]
pub struct Take {
    pub target: MachineId,
    pub landing: Option<Landing>,
    pub resistance_px: u32,
    pub reach: Vec<MachineId>,
}

/// Where this machine's physical input is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    Home,
    Driving(MachineId),
}

/// Effects to execute in order. Releases precede accepting a replacement owner.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Send {
        peer: MachineId,
        body: Body,
    },
    Release(Vec<InputEvent>),
    Cursor(Cursor),
    /// A new remote owner; `landing` is absent for a stay re-arm.
    Driven {
        owner: MachineId,
        landing: Option<Landing>,
        /// The owner's raw value; pass it to `edge::EdgeResistance::new`.
        resistance_px: u32,
    },
}

#[derive(Debug, Clone)]
struct Session {
    take: Take,
    route: Route,
}

#[derive(Debug)]
enum Owner {
    Home,
    Driving(Session),
    Taking {
        session: Session,
        previous: Option<Session>,
        deadline: Instant,
    },
}

#[derive(Debug)]
struct Driven {
    owner: MachineId,
    route: Route,
    last_input: Instant,
    reach: Vec<MachineId>,
}

/// Both roles live together so local activity and remote ownership agree.
#[derive(Debug)]
pub struct Focus {
    own_id: MachineId,
    last_route: Route,
    owner: Owner,
    driven: Option<Driven>,
    accepted: BTreeMap<MachineId, Route>,
    last_local_input: Option<Instant>,
    held: HeldInputs,
}

impl Focus {
    #[must_use]
    pub fn new(own_id: MachineId) -> Self {
        Self {
            own_id,
            last_route: Route(0),
            owner: Owner::Home,
            driven: None,
            accepted: BTreeMap::new(),
            last_local_input: None,
            held: HeldInputs::default(),
        }
    }

    #[must_use]
    pub fn cursor(&self) -> Cursor {
        match &self.owner {
            Owner::Home => Cursor::Home,
            Owner::Driving(session) => Cursor::Driving(session.take.target),
            Owner::Taking { previous, .. } => previous
                .as_ref()
                .map_or(Cursor::Home, |s| Cursor::Driving(s.take.target)),
        }
    }

    /// The active owner's route, including the previous route during a take.
    #[must_use]
    pub fn driving_route(&self) -> Option<Route> {
        match &self.owner {
            Owner::Home => None,
            Owner::Driving(session) => Some(session.route),
            Owner::Taking { previous, .. } => previous.as_ref().map(|s| s.route),
        }
    }

    /// Whether physical input is currently suppressed by a remote owner.
    #[must_use]
    pub fn is_driven(&self) -> bool {
        self.driven.is_some()
    }

    /// None when there is no take awaiting an answer.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        match self.owner {
            Owner::Taking { deadline, .. } => Some(deadline),
            _ => None,
        }
    }

    fn next_route(&mut self) -> Option<Route> {
        // Routes must never wrap into an older route.
        self.last_route = Route(self.last_route.0.checked_add(1)?);
        Some(self.last_route)
    }

    fn send(peer: MachineId, body: Body) -> Action {
        Action::Send { peer, body }
    }

    fn focus_message(session: &Session, target: MachineId, stay: bool) -> WireFocus {
        WireFocus {
            route: session.route.0,
            target: target.as_bytes().to_vec(),
            landing: if stay {
                None
            } else {
                session.take.landing.clone()
            },
            resistance_px: session.take.resistance_px,
            reach: session
                .take
                .reach
                .iter()
                .map(|id| id.as_bytes().to_vec())
                .collect(),
            stay,
        }
    }
}

#[cfg(test)]
mod tests;
