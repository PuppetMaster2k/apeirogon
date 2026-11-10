// SPDX-License-Identifier: GPL-3.0-or-later

use std::ffi::OsStr;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, Window};
use x11rb::rust_connection::RustConnection;

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        active_window: b"_NET_ACTIVE_WINDOW".as_slice(),
        state: b"_NET_WM_STATE".as_slice(),
        fullscreen: b"_NET_WM_STATE_FULLSCREEN".as_slice(),
        pid: b"_NET_WM_PID".as_slice(),
    }
}

const MAX_WINDOW_STATES: u32 = 1024;

pub(super) fn focused_fullscreen_at(x: i32, y: i32) -> Option<bool> {
    let (connection, screen) = x11rb::connect(None).ok()?;
    let root = connection.setup().roots.get(screen)?.root;
    let atoms = Atoms::new(&connection).ok()?.reply().ok()?;
    let active = connection
        .get_property(false, root, atoms.active_window, AtomEnum::WINDOW, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    let Some(window) = active
        .value32()
        .and_then(|mut values| values.next())
        .filter(|window| *window != x11rb::NONE)
    else {
        return Some(false);
    };
    let states = connection
        .get_property(
            false,
            window,
            atoms.state,
            AtomEnum::ATOM,
            0,
            MAX_WINDOW_STATES,
        )
        .ok()?
        .reply()
        .ok()?;
    if states.bytes_after != 0
        || !states
            .value32()
            .is_some_and(|mut values| values.any(|atom| atom == atoms.fullscreen))
    {
        return Some(false);
    }
    if is_our_gui(&connection, window, atoms.pid)? {
        return Some(false);
    }
    let geometry = connection.get_geometry(window).ok()?.reply().ok()?;
    let origin = connection
        .translate_coordinates(window, root, 0, 0)
        .ok()?
        .reply()
        .ok()?;
    let left = i32::from(origin.dst_x);
    let top = i32::from(origin.dst_y);
    Some(
        x >= left
            && y >= top
            && x < left + i32::from(geometry.width)
            && y < top + i32::from(geometry.height),
    )
}

fn is_our_gui(connection: &RustConnection, window: Window, pid_atom: u32) -> Option<bool> {
    let property = connection
        .get_property(false, window, pid_atom, AtomEnum::CARDINAL, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    let Some(pid) = property.value32().and_then(|mut values| values.next()) else {
        return Some(false);
    };
    let executable = std::fs::read_link(format!("/proc/{pid}/exe")).ok();
    Some(executable.is_some_and(|path| path.file_name() == Some(OsStr::new(super::GUI_NAME))))
}
