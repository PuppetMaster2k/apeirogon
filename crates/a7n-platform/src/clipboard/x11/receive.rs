// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Instant;

use a7n_core::clipboard::{Item, LocalChange};
use a7n_proto::limits::CLIPBOARD_TEXT_MAX;
use tokio::sync::mpsc::Sender;
use x11rb::NONE;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, Property, Window};

use super::{CONVERSION_TIMEOUT, Session, create_window, mime, select_targets};

#[derive(Default)]
pub(super) struct Receiver {
    active: Option<Read>,
    ready: Option<LocalChange>,
}

struct Read {
    window: Window,
    owner: Window,
    time: u32,
    target: u32,
    remaining: Vec<u32>,
    change: LocalChange,
    data: Vec<u8>,
    incremental: bool,
    deadline: Instant,
}

impl Receiver {
    pub(super) fn cancel(&mut self, session: &Session) {
        if let Some(read) = self.active.take() {
            let _ = session.conn.destroy_window(read.window);
        }
        self.ready = None;
    }

    pub(super) fn begin(&mut self, session: &Session, owner: Window, time: u32) {
        self.cancel(session);
        if owner == NONE || owner == session.window {
            return;
        }
        let Ok(window) = create_window(&session.conn, session.root) else {
            return;
        };
        self.active = Some(Read {
            window,
            owner,
            time,
            target: session.atoms.TARGETS,
            remaining: Vec::new(),
            change: LocalChange {
                items: Vec::new(),
                sensitive: false,
                source_app: source_app(session, owner),
            },
            data: Vec::new(),
            incremental: false,
            deadline: Instant::now() + CONVERSION_TIMEOUT,
        });
        self.convert(session);
    }

    fn convert(&self, session: &Session) {
        if let Some(read) = &self.active {
            let _ = session.conn.convert_selection(
                read.window,
                session.atoms.CLIPBOARD,
                read.target,
                session.atoms.PROPERTY,
                read.time,
            );
        }
    }

    pub(super) fn event(&mut self, session: &Session, event: &Event) {
        let Some(read) = &self.active else {
            return;
        };
        match event {
            Event::SelectionNotify(event)
                if event.requestor == read.window
                    && event.selection == session.atoms.CLIPBOARD
                    && event.target == read.target =>
            {
                if event.property == NONE {
                    self.advance(session);
                } else if event.property == session.atoms.PROPERTY {
                    self.property(session, false);
                }
            }
            Event::PropertyNotify(event)
                if event.window == read.window
                    && event.atom == session.atoms.PROPERTY
                    && event.state == Property::NEW_VALUE
                    && read.incremental =>
            {
                self.property(session, true);
            }
            _ => {}
        }
    }

    fn property(&mut self, session: &Session, chunk: bool) {
        let Some(read) = &mut self.active else {
            return;
        };
        // Bound the reply before allocating, including a hostile TARGETS list.
        let reply = session
            .conn
            .get_property(
                true,
                read.window,
                session.atoms.PROPERTY,
                AtomEnum::ANY,
                0,
                u32::try_from(CLIPBOARD_TEXT_MAX / 4 + 1).unwrap_or(u32::MAX),
            )
            .ok()
            .and_then(|cookie| cookie.reply().ok());
        let Some(reply) = reply else {
            self.advance(session);
            return;
        };
        if reply.bytes_after != 0 || reply.value.len() > CLIPBOARD_TEXT_MAX {
            self.advance(session);
            return;
        }
        if !chunk && reply.type_ == session.atoms.INCR {
            let size = reply.value32().and_then(|mut values| values.next());
            if read.target == session.atoms.TARGETS
                || reply.format != 32
                || size.is_none_or(|size| u64::from(size) > CLIPBOARD_TEXT_MAX as u64)
            {
                self.advance(session);
                return;
            }
            read.incremental = true;
            return;
        }
        if read.target == session.atoms.TARGETS {
            if reply.type_ != u32::from(AtomEnum::ATOM) || reply.format != 32 {
                self.advance(session);
                return;
            }
            // TARGETS is normally tiny; accept INCR for text only.
            let targets: Vec<_> = reply.value32().into_iter().flatten().collect();
            if targets.contains(&session.atoms.SENSITIVE) {
                read.change.sensitive = true;
                read.remaining.clear();
            } else {
                read.remaining = select_targets(&session.atoms, &targets);
                read.remaining.reverse();
            }
            self.advance(session);
            return;
        }
        if reply.format != 8
            || reply.type_ != read.target
            || read.data.len().saturating_add(reply.value.len()) > CLIPBOARD_TEXT_MAX
        {
            self.advance(session);
            return;
        }
        read.data.extend_from_slice(&reply.value);
        if !chunk || reply.value.is_empty() {
            if let Some(mime) = mime(&session.atoms, read.target) {
                read.change.items.push(Item {
                    mime: mime.into(),
                    data: std::mem::take(&mut read.data),
                });
            }
            self.advance(session);
        }
    }

    fn advance(&mut self, session: &Session) {
        let Some(mut read) = self.active.take() else {
            return;
        };
        if let Some(target) = read.remaining.pop() {
            read.target = target;
            read.data.clear();
            read.incremental = false;
            read.deadline = Instant::now() + CONVERSION_TIMEOUT;
            // A fresh requestor window isolates late replies from the next format.
            let _ = session.conn.destroy_window(read.window);
            let Ok(window) = create_window(&session.conn, session.root) else {
                return;
            };
            read.window = window;
            self.active = Some(read);
            self.convert(session);
        } else {
            let _ = session.conn.destroy_window(read.window);
            // Never publish data read while ownership was changing.
            let owner = session
                .conn
                .get_selection_owner(session.atoms.CLIPBOARD)
                .ok()
                .and_then(|cookie| cookie.reply().ok());
            if owner.is_some_and(|owner| owner.owner == read.owner) {
                self.ready = Some(read.change);
            }
        }
    }

    pub(super) fn tick(&mut self, session: &Session, changes: &Sender<LocalChange>, now: Instant) {
        if self
            .active
            .as_ref()
            .is_some_and(|read| now >= read.deadline)
        {
            self.advance(session);
        }
        if let Some(change) = self.ready.take() {
            if let Err(tokio::sync::mpsc::error::TrySendError::Full(change)) =
                changes.try_send(change)
            {
                self.ready = Some(change);
            }
        }
    }
}

fn source_app(session: &Session, owner: Window) -> Option<String> {
    // ponytail: WM_CLASS may be absent on selection windows; upgrade to client
    // leader lookup if applications need more reliable deny-list attribution.
    let reply = session
        .conn
        .get_property(
            false,
            owner,
            session.atoms.WM_CLASS,
            AtomEnum::STRING,
            0,
            1024,
        )
        .ok()?
        .reply()
        .ok()?;
    let class = reply.value.split(|byte| *byte == 0).nth(1)?;
    (!class.is_empty()).then(|| String::from_utf8_lossy(class).into_owned())
}
