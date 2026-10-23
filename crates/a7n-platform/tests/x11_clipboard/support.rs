// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Mutex;
use std::time::{Duration, Instant};

use a7n_core::clipboard::{Generation, Item, LocalChange};
use a7n_platform::clipboard::{Clipboard, ClipboardCommand, LazyItem};
use a7n_proto::MachineId;
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use tokio::sync::mpsc;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ConnectionExt as _, CreateWindowAux, EventMask, PropMode,
    Property, SELECTION_NOTIFY_EVENT, SelectionNotifyEvent, SelectionRequestEvent, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE};

const DEADLINE: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(5);
pub(super) const LARGE_SIZE: usize = 200 * 1024;
const CHUNK: usize = 32 * 1024;
pub(super) static SELECTION: Mutex<()> = Mutex::new(());

x11rb::atom_manager! {
    pub(super) Atoms: AtomsCookie {
        CLIPBOARD, TARGETS, UTF8_STRING, INCR, WM_CLASS,
        HTML: b"text/html",
        SENSITIVE: b"x-kde-passwordManagerHint",
        PROPERTY: b"_APEIROGON_TEST_CLIPBOARD",
    }
}

pub(super) struct Client {
    conn: RustConnection,
    window: u32,
    pub(super) atoms: Atoms,
    pub(super) reads: usize,
    outgoing: Option<(SelectionRequestEvent, Vec<u8>, usize)>,
}

impl Client {
    pub(super) fn new() -> Self {
        let (conn, screen) = x11rb::connect(None).unwrap();
        let window = conn.generate_id().unwrap();
        conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            conn.setup().roots[screen].root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )
        .unwrap()
        .check()
        .unwrap();
        let atoms = Atoms::new(&conn).unwrap().reply().unwrap();
        Self {
            conn,
            window,
            atoms,
            reads: 0,
            outgoing: None,
        }
    }

    pub(super) fn own(&self) {
        self.conn
            .change_property8(
                PropMode::REPLACE,
                self.window,
                self.atoms.WM_CLASS,
                AtomEnum::STRING,
                b"test-instance\0ClipboardTest\0",
            )
            .unwrap();
        self.conn
            .set_selection_owner(self.window, self.atoms.CLIPBOARD, CURRENT_TIME)
            .unwrap()
            .check()
            .unwrap();
        self.conn.flush().unwrap();
    }

    fn wait(&self) {
        self.conn.flush().unwrap();
        let mut fds = [PollFd::new(self.conn.stream(), PollFlags::IN)];
        poll(&mut fds, Some(&Timespec::try_from(POLL_INTERVAL).unwrap())).unwrap();
    }

    fn notify(&self, request: &SelectionRequestEvent) {
        let event = SelectionNotifyEvent {
            response_type: SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: request.time,
            requestor: request.requestor,
            selection: request.selection,
            target: request.target,
            property: request.property,
        };
        self.conn
            .send_event(false, request.requestor, EventMask::NO_EVENT, event)
            .unwrap();
    }

    fn serve(&mut self, event: &Event, data: &[u8], sensitive: bool, incr: bool) {
        match event {
            Event::SelectionRequest(request) => {
                if request.target == self.atoms.TARGETS {
                    let targets = if sensitive {
                        vec![self.atoms.UTF8_STRING, self.atoms.SENSITIVE]
                    } else {
                        vec![self.atoms.UTF8_STRING, self.atoms.HTML]
                    };
                    self.conn
                        .change_property32(
                            PropMode::REPLACE,
                            request.requestor,
                            request.property,
                            AtomEnum::ATOM,
                            &targets,
                        )
                        .unwrap();
                } else {
                    self.reads += 1;
                    if incr {
                        self.conn
                            .change_window_attributes(
                                request.requestor,
                                &ChangeWindowAttributesAux::new()
                                    .event_mask(EventMask::PROPERTY_CHANGE),
                            )
                            .unwrap();
                        self.conn
                            .change_property32(
                                PropMode::REPLACE,
                                request.requestor,
                                request.property,
                                self.atoms.INCR,
                                &[u32::try_from(data.len()).unwrap()],
                            )
                            .unwrap();
                        self.outgoing = Some((*request, data.to_vec(), 0));
                    } else {
                        self.conn
                            .change_property8(
                                PropMode::REPLACE,
                                request.requestor,
                                request.property,
                                request.target,
                                data,
                            )
                            .unwrap();
                    }
                }
                self.notify(request);
            }
            Event::PropertyNotify(event) if event.state == Property::DELETE => {
                if let Some((request, data, offset)) = &mut self.outgoing {
                    if event.window == request.requestor && event.atom == request.property {
                        let end = (*offset + CHUNK).min(data.len());
                        self.conn
                            .change_property8(
                                PropMode::REPLACE,
                                request.requestor,
                                request.property,
                                request.target,
                                &data[*offset..end],
                            )
                            .unwrap();
                        let done = *offset == data.len();
                        *offset = end;
                        if done {
                            self.outgoing = None;
                        }
                    }
                }
            }
            _ => {}
        }
        self.conn.flush().unwrap();
    }

    pub(super) fn change(
        &mut self,
        clipboard: &mut Clipboard,
        data: &[u8],
        sensitive: bool,
        incr: bool,
    ) -> LocalChange {
        let deadline = Instant::now() + DEADLINE;
        loop {
            if let Ok(change) = clipboard.changes.try_recv() {
                return change;
            }
            assert!(Instant::now() < deadline, "local change timed out");
            while let Some(event) = self.conn.poll_for_event().unwrap() {
                self.serve(&event, data, sensitive, incr);
            }
            self.wait();
        }
    }

    pub(super) fn convert(&self, target: u32) {
        self.conn
            .convert_selection(
                self.window,
                self.atoms.CLIPBOARD,
                target,
                self.atoms.PROPERTY,
                CURRENT_TIME,
            )
            .unwrap();
        self.conn.flush().unwrap();
    }

    pub(super) fn read(&self) -> (Vec<u8>, bool) {
        let deadline = Instant::now() + DEADLINE;
        let mut incremental = false;
        let mut data = Vec::new();
        loop {
            assert!(Instant::now() < deadline, "selection read timed out");
            while let Some(event) = self.conn.poll_for_event().unwrap() {
                let read = match event {
                    Event::SelectionNotify(event) => {
                        assert_ne!(event.property, NONE, "selection refused");
                        true
                    }
                    Event::PropertyNotify(event) => {
                        incremental && event.state == Property::NEW_VALUE
                    }
                    _ => false,
                };
                if !read {
                    continue;
                }
                let reply = self
                    .conn
                    .get_property(
                        true,
                        self.window,
                        self.atoms.PROPERTY,
                        AtomEnum::ANY,
                        0,
                        u32::MAX,
                    )
                    .unwrap()
                    .reply()
                    .unwrap();
                if reply.type_ == self.atoms.INCR {
                    incremental = true;
                } else {
                    let done = !incremental || reply.value.is_empty();
                    data.extend_from_slice(&reply.value);
                    if done {
                        return (data, incremental);
                    }
                }
                self.conn.flush().unwrap();
            }
            self.wait();
        }
    }
}

pub(super) fn generation() -> Generation {
    Generation {
        generation: 1,
        origin: MachineId::try_from(&[1; 16][..]).unwrap(),
    }
}

pub(super) fn set(clipboard: &Clipboard, items: Vec<Item>, lazy: Vec<LazyItem>) {
    clipboard
        .commands
        .blocking_send(ClipboardCommand::Set {
            generation: generation(),
            items,
            lazy,
        })
        .unwrap();
}

pub(super) fn wait_owned(client: &Client) {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let owner = client
            .conn
            .get_selection_owner(client.atoms.CLIPBOARD)
            .unwrap()
            .reply()
            .unwrap()
            .owner;
        if owner != NONE && owner != client.window {
            return;
        }
        assert!(Instant::now() < deadline, "backend did not take ownership");
        client.wait();
    }
}

pub(super) fn receive<T>(client: &Client, channel: &mut mpsc::Receiver<T>) -> T {
    let deadline = Instant::now() + DEADLINE;
    loop {
        if let Ok(value) = channel.try_recv() {
            return value;
        }
        assert!(Instant::now() < deadline, "channel timed out");
        client.wait();
    }
}
