// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Instant;

use a7n_core::clipboard::{Generation, Item};
use tokio::sync::{mpsc::Sender, oneshot};
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ConnectionExt as _, EventMask, PropMode, Property,
    SELECTION_NOTIFY_EVENT, SelectionNotifyEvent, SelectionRequestEvent,
};
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{CURRENT_TIME, NONE};

use super::{MAX_PENDING, PASTE_TIMEOUT, Session, mime};
use crate::clipboard::{ClipboardCommand, LazyItem, PasteRequest};

#[derive(Default)]
pub(super) struct Owner {
    generation: Option<Generation>,
    items: Vec<Item>,
    lazy: Vec<LazyItem>,
    pending: Vec<Pending>,
    outgoing: Vec<Outgoing>,
}

struct Pending {
    request: SelectionRequestEvent,
    reply: oneshot::Receiver<Option<Vec<u8>>>,
    deadline: Instant,
}

struct Outgoing {
    request: SelectionRequestEvent,
    data: Vec<u8>,
    offset: usize,
    deadline: Instant,
}

impl Owner {
    pub(super) fn clear(&mut self, session: &Session) {
        for pending in self.pending.drain(..) {
            notify(session, &pending.request, NONE);
        }
        self.generation = None;
        self.items.clear();
        self.lazy.clear();
        self.outgoing.clear();
    }

    pub(super) fn set(&mut self, session: &Session, command: ClipboardCommand) {
        self.clear(session);
        let ClipboardCommand::Set {
            generation,
            items,
            lazy,
        } = command;
        self.generation = Some(generation);
        self.items = items
            .into_iter()
            .filter(|item| item.data.len() <= a7n_core::clipboard::limit_for(&item.mime))
            .collect();
        self.lazy = lazy
            .into_iter()
            .filter(|item| item.size <= a7n_core::clipboard::limit_for(&item.mime) as u64)
            .collect();
        let _ =
            session
                .conn
                .set_selection_owner(session.window, session.atoms.CLIPBOARD, CURRENT_TIME);
    }

    pub(super) fn event(
        &mut self,
        session: &Session,
        event: &Event,
        pastes: &Sender<PasteRequest>,
    ) {
        match event {
            Event::SelectionRequest(request) if request.selection == session.atoms.CLIPBOARD => {
                self.request(session, *request, pastes);
            }
            Event::PropertyNotify(event) if event.state == Property::DELETE => {
                self.chunk(session, event.window, event.atom);
            }
            // XFixes notifies ownership changes; SelectionClear can be queued
            // from an older Set, so it must not clear a newer generation.
            _ => {}
        }
    }

    fn request(
        &mut self,
        session: &Session,
        mut request: SelectionRequestEvent,
        pastes: &Sender<PasteRequest>,
    ) {
        if request.property == NONE {
            request.property = request.target;
        }
        let owner = session
            .conn
            .get_selection_owner(session.atoms.CLIPBOARD)
            .ok()
            .and_then(|cookie| cookie.reply().ok());
        if self.generation.is_none() || owner.is_none_or(|owner| owner.owner != session.window) {
            notify(session, &request, NONE);
            return;
        }
        if request.target == session.atoms.TARGETS {
            let targets = self.targets(session);
            let success = session
                .conn
                .change_property32(
                    PropMode::REPLACE,
                    request.requestor,
                    request.property,
                    AtomEnum::ATOM,
                    &targets,
                )
                .is_ok_and(|cookie| cookie.check().is_ok());
            notify(
                session,
                &request,
                if success { request.property } else { NONE },
            );
            return;
        }
        let Some(mime) = mime(&session.atoms, request.target) else {
            notify(session, &request, NONE);
            return;
        };
        if let Some(item) = self.items.iter().find(|item| item.mime == mime) {
            self.respond(session, request, item.data.clone());
            return;
        }
        if self.pending.len() < MAX_PENDING {
            if let Some(item) = self.lazy.iter().find(|item| item.mime == mime) {
                let (reply, receiver) = oneshot::channel();
                let paste = PasteRequest {
                    generation: self.generation.expect("owned selection has a generation"),
                    item_index: item.item_index,
                    reply,
                };
                if pastes.try_send(paste).is_ok() {
                    self.pending.push(Pending {
                        request,
                        reply: receiver,
                        deadline: Instant::now() + PASTE_TIMEOUT,
                    });
                    return;
                }
            }
        }
        notify(session, &request, NONE);
    }

    fn targets(&self, session: &Session) -> Vec<u32> {
        let mut targets = vec![session.atoms.TARGETS];
        for target in [
            session.atoms.UTF8_STRING,
            session.atoms.PLAIN,
            session.atoms.HTML,
            session.atoms.HTML_UTF8,
            session.atoms.PNG,
        ] {
            if mime(&session.atoms, target).is_some_and(|mime| {
                self.items.iter().any(|item| item.mime == mime)
                    || self.lazy.iter().any(|item| item.mime == mime)
            }) {
                targets.push(target);
            }
        }
        targets
    }

    fn respond(&mut self, session: &Session, request: SelectionRequestEvent, data: Vec<u8>) {
        if data.len()
            > a7n_core::clipboard::limit_for(mime(&session.atoms, request.target).unwrap_or(""))
            || session.chunk_size == 0
        {
            notify(session, &request, NONE);
            return;
        }
        let success = if data.len() <= session.chunk_size {
            session
                .conn
                .change_property8(
                    PropMode::REPLACE,
                    request.requestor,
                    request.property,
                    request.target,
                    &data,
                )
                .is_ok_and(|cookie| cookie.check().is_ok())
        } else {
            self.begin_incremental(session, request, data)
        };
        notify(
            session,
            &request,
            if success { request.property } else { NONE },
        );
    }

    fn begin_incremental(
        &mut self,
        session: &Session,
        request: SelectionRequestEvent,
        data: Vec<u8>,
    ) -> bool {
        if self.outgoing.len() >= MAX_PENDING
            || self.outgoing.iter().any(|outgoing| {
                outgoing.request.requestor == request.requestor
                    && outgoing.request.property == request.property
            })
        {
            return false;
        }
        let Ok(cookie) = session.conn.change_window_attributes(
            request.requestor,
            &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        ) else {
            return false;
        };
        if cookie.check().is_err() {
            return false;
        }
        let Ok(size) = u32::try_from(data.len()) else {
            return false;
        };
        let success = session
            .conn
            .change_property32(
                PropMode::REPLACE,
                request.requestor,
                request.property,
                session.atoms.INCR,
                &[size],
            )
            .is_ok_and(|cookie| cookie.check().is_ok());
        if success {
            self.outgoing.push(Outgoing {
                request,
                data,
                offset: 0,
                deadline: Instant::now() + PASTE_TIMEOUT,
            });
        }
        success
    }

    fn chunk(&mut self, session: &Session, window: u32, property: u32) {
        let Some(index) = self.outgoing.iter().position(|outgoing| {
            outgoing.request.requestor == window && outgoing.request.property == property
        }) else {
            return;
        };
        let outgoing = &mut self.outgoing[index];
        let end = outgoing
            .offset
            .saturating_add(session.chunk_size)
            .min(outgoing.data.len());
        let success = session
            .conn
            .change_property8(
                PropMode::REPLACE,
                window,
                property,
                outgoing.request.target,
                &outgoing.data[outgoing.offset..end],
            )
            .is_ok_and(|cookie| cookie.check().is_ok());
        let done = outgoing.offset == outgoing.data.len();
        outgoing.offset = end;
        if done || !success {
            self.outgoing.swap_remove(index);
        }
    }

    pub(super) fn tick(&mut self, session: &Session, now: Instant) {
        let mut index = 0;
        while index < self.pending.len() {
            let pending = &mut self.pending[index];
            let reply = if now >= pending.deadline {
                Ok(None)
            } else {
                pending.reply.try_recv()
            };
            match reply {
                Err(oneshot::error::TryRecvError::Empty) => index += 1,
                result => {
                    let pending = self.pending.swap_remove(index);
                    if let Ok(Some(data)) = result {
                        self.respond(session, pending.request, data);
                    } else {
                        notify(session, &pending.request, NONE);
                    }
                }
            }
        }
        self.outgoing.retain(|outgoing| now < outgoing.deadline);
    }
}

fn notify(session: &Session, request: &SelectionRequestEvent, property: u32) {
    let event = SelectionNotifyEvent {
        response_type: SELECTION_NOTIFY_EVENT,
        sequence: 0,
        time: request.time,
        requestor: request.requestor,
        selection: request.selection,
        target: request.target,
        property,
    };
    let _ = session
        .conn
        .send_event(false, request.requestor, EventMask::NO_EVENT, event);
}
