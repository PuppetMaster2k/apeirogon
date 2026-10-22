// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use a7n_proto::limits;
use a7n_proto::v1::{Accept, Ack, Handover, KeyEvent, Refuse, RefuseReason, input_event};

mod owner;
mod responder;

fn id(byte: u8) -> MachineId {
    MachineId::try_from([byte; MachineId::LEN].as_slice()).unwrap()
}

fn request(target: u8) -> Take {
    Take {
        target: id(target),
        landing: None,
        resistance_px: 0,
        reach: vec![id(2), id(3)],
    }
}

fn wire(route: u64) -> WireFocus {
    WireFocus {
        route,
        target: id(1).as_bytes().to_vec(),
        reach: vec![id(3).as_bytes().to_vec()],
        ..Default::default()
    }
}

fn driving(now: Instant) -> Focus {
    let mut focus = Focus::new(id(1));
    focus.take(request(2), now);
    focus.on_accept(id(2), &Accept { route: 1 });
    focus
}

fn held_key() -> InputEvent {
    InputEvent {
        event: Some(input_event::Event::Key(KeyEvent {
            usage: 7,
            down: true,
            character: String::new(),
        })),
    }
}

fn has_let_go(actions: &[Action], peer: u8, target: u8) -> bool {
    actions.iter().any(|a| matches!(a, Action::Send { peer: p, body: Body::Focus(f) } if *p == id(peer) && f.target == id(target).as_bytes()))
}

fn released_key(actions: &[Action]) -> bool {
    actions.iter().any(|a| matches!(a, Action::Release(events) if events.len() == 1 && matches!(&events[0].event, Some(input_event::Event::Key(key)) if !key.down && key.usage == 7)))
}
