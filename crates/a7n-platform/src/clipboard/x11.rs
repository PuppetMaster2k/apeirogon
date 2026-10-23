// SPDX-License-Identifier: GPL-3.0-or-later

//! `XFixes` clipboard monitoring and delayed selection rendering on one connection.

mod receive;
mod serve;

use std::time::{Duration, Instant};

use a7n_core::clipboard::{TEXT_HTML, TEXT_PLAIN};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use tokio::sync::mpsc;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::Event;
use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEventMask};
use x11rb::protocol::xproto::{
    ConnectionExt as _, CreateWindowAux, EventMask, Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME};

use super::Clipboard;
use super::formats::{HTML_ALIAS, HTML_UTF8_ALIAS, SENSITIVE_MARKER};
use crate::input::InputError;

const CHANNEL_CAPACITY: usize = 16;
const WAKE_INTERVAL: Duration = Duration::from_millis(10);
const CONVERSION_TIMEOUT: Duration = Duration::from_secs(1);
const PASTE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_PENDING: usize = 16;
// Keep large properties incremental even on servers supporting BIG-REQUESTS.
const MAX_PROPERTY_CHUNK: usize = 64 * 1024;

x11rb::atom_manager! {
    pub Atoms: AtomsCookie {
        CLIPBOARD, TARGETS, UTF8_STRING, INCR, WM_CLASS,
        PLAIN: TEXT_PLAIN.as_bytes(),
        HTML: HTML_ALIAS.as_bytes(),
        HTML_UTF8: HTML_UTF8_ALIAS.as_bytes(),
        SENSITIVE: SENSITIVE_MARKER.as_bytes(),
        PROPERTY: b"_APEIROGON_CLIPBOARD",
    }
}

struct Session {
    conn: RustConnection,
    window: Window,
    root: Window,
    atoms: Atoms,
    chunk_size: usize,
}

impl Session {
    fn new() -> Result<Self, InputError> {
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        conn.xfixes_query_version(2, 0)?.reply()?;
        let atoms = Atoms::new(&conn)?.reply()?;
        let window = create_window(&conn, root)?;
        conn.xfixes_select_selection_input(
            window,
            atoms.CLIPBOARD,
            SelectionEventMask::SET_SELECTION_OWNER
                | SelectionEventMask::SELECTION_WINDOW_DESTROY
                | SelectionEventMask::SELECTION_CLIENT_CLOSE,
        )?
        .check()?;
        let chunk_size = chunk_size(conn.maximum_request_bytes());
        conn.flush()?;
        Ok(Self {
            conn,
            window,
            root,
            atoms,
            chunk_size,
        })
    }
}

fn create_window(conn: &RustConnection, root: Window) -> Result<Window, InputError> {
    let window = conn
        .generate_id()
        .map_err(|error| InputError::Thread(std::io::Error::other(error)))?;
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        window,
        root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )?
    .check()?;
    Ok(window)
}

pub(super) fn start() -> Result<Clipboard, InputError> {
    let session = Session::new()?;
    let (changes_tx, changes) = mpsc::channel(CHANNEL_CAPACITY);
    let (commands, mut commands_rx) = mpsc::channel(CHANNEL_CAPACITY);
    let (pastes_tx, pastes) = mpsc::channel(CHANNEL_CAPACITY);
    std::thread::Builder::new()
        .name("a7n-x11-clipboard".into())
        .spawn(move || {
            let mut receive = receive::Receiver::default();
            let mut serve = serve::Owner::default();
            // Also discover a selection that predates our subscription.
            if let Ok(cookie) = session.conn.get_selection_owner(session.atoms.CLIPBOARD) {
                if let Ok(reply) = cookie.reply() {
                    receive.begin(&session, reply.owner, CURRENT_TIME);
                }
            }
            loop {
                match commands_rx.try_recv() {
                    Ok(command) => {
                        receive.cancel(&session);
                        serve.set(&session, command);
                    }
                    Err(mpsc::error::TryRecvError::Disconnected) => break,
                    Err(mpsc::error::TryRecvError::Empty) => {}
                }
                // A busy X client must not starve command processing or deadlines.
                for _ in 0..CHANNEL_CAPACITY {
                    match session.conn.poll_for_event() {
                        Ok(Some(Event::XfixesSelectionNotify(event))) => {
                            let current = session
                                .conn
                                .get_selection_owner(session.atoms.CLIPBOARD)
                                .ok()
                                .and_then(|cookie| cookie.reply().ok());
                            if event.owner != session.window
                                && current.is_some_and(|reply| reply.owner == event.owner)
                            {
                                serve.clear(&session);
                                receive.begin(&session, event.owner, event.selection_timestamp);
                            }
                        }
                        Ok(Some(event)) => {
                            receive.event(&session, &event);
                            serve.event(&session, &event, &pastes_tx);
                        }
                        Ok(None) => break,
                        Err(_) => return,
                    }
                }
                let now = Instant::now();
                receive.tick(&session, &changes_tx, now);
                serve.tick(&session, now);
                if session.conn.flush().is_err() {
                    break;
                }
                let mut fds = [PollFd::new(session.conn.stream(), PollFlags::IN)];
                if let Ok(timeout) = Timespec::try_from(WAKE_INTERVAL) {
                    let _ = poll(&mut fds, Some(&timeout));
                }
            }
        })
        .map_err(InputError::Thread)?;
    Ok(Clipboard {
        changes,
        commands,
        pastes,
    })
}

fn mime(atoms: &Atoms, target: u32) -> Option<&'static str> {
    if target == atoms.UTF8_STRING || target == atoms.PLAIN {
        Some(TEXT_PLAIN)
    } else if target == atoms.HTML || target == atoms.HTML_UTF8 {
        Some(TEXT_HTML)
    } else {
        None
    }
}

// Leave room for ChangeProperty's header and the optional BIG-REQUESTS length.
fn chunk_size(maximum_request_bytes: usize) -> usize {
    maximum_request_bytes
        .saturating_sub(32)
        .min(MAX_PROPERTY_CHUNK)
        & !3
}

fn select_targets(atoms: &Atoms, targets: &[u32]) -> Vec<u32> {
    let mut selected = Vec::new();
    if let Some(target) = [atoms.UTF8_STRING, atoms.PLAIN]
        .into_iter()
        .find(|t| targets.contains(t))
    {
        selected.push(target);
    }
    if let Some(target) = [atoms.HTML, atoms.HTML_UTF8]
        .into_iter()
        .find(|t| targets.contains(t))
    {
        selected.push(target);
    }
    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atoms() -> Atoms {
        Atoms {
            CLIPBOARD: 1,
            TARGETS: 2,
            UTF8_STRING: 3,
            INCR: 4,
            WM_CLASS: 5,
            PLAIN: 6,
            HTML: 7,
            HTML_UTF8: 8,
            SENSITIVE: 9,
            PROPERTY: 10,
        }
    }

    #[test]
    fn select_targets_aliases_prefers_utf8_and_maps_supported_formats() {
        // Arrange
        let atoms = atoms();
        let cases = [
            (
                vec![atoms.PLAIN, atoms.UTF8_STRING, atoms.HTML],
                vec![atoms.UTF8_STRING, atoms.HTML],
            ),
            (
                vec![atoms.PLAIN, atoms.HTML_UTF8],
                vec![atoms.PLAIN, atoms.HTML_UTF8],
            ),
            (vec![atoms.SENSITIVE], vec![]),
            (vec![], vec![]),
        ];
        for (offered, expected) in cases {
            // Act
            let selected = select_targets(&atoms, &offered);

            // Assert
            assert_eq!(selected, expected);
        }
    }

    #[test]
    fn mime_target_aliases_maps_only_supported_formats() {
        // Arrange
        let atoms = atoms();
        let cases = [
            (atoms.UTF8_STRING, Some(TEXT_PLAIN)),
            (atoms.PLAIN, Some(TEXT_PLAIN)),
            (atoms.HTML, Some(TEXT_HTML)),
            (atoms.HTML_UTF8, Some(TEXT_HTML)),
            (atoms.SENSITIVE, None),
        ];
        for (target, expected) in cases {
            // Act
            let mapped = mime(&atoms, target);

            // Assert
            assert_eq!(mapped, expected);
        }
    }

    #[test]
    fn chunk_size_request_boundary_leaves_header_and_alignment() {
        // Arrange
        let cases = [
            (0, 0),
            (31, 0),
            (35, 0),
            (36, 4),
            (1024, 992),
            (262_140, MAX_PROPERTY_CHUNK),
        ];
        for (maximum, expected) in cases {
            // Act
            let chunk = chunk_size(maximum);

            // Assert
            assert_eq!(chunk, expected);
        }
    }
}
