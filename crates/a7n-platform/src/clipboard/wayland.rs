// SPDX-License-Identifier: GPL-3.0-or-later

//! Wayland clipboard: data-control where the compositor offers it, else the
//! clipboard portal on the `RemoteDesktop` session of the input backend.
//!
//! - `ext_data_control_manager_v1` (preferred) or `zwlr_data_control_manager_v1`
//!   on wlroots and KDE: one extra Wayland connection, no consent dialog.
//! - GNOME offers no data-control. The input backend asks for clipboard access
//!   before it starts the `RemoteDesktop` session and hands the resulting
//!   [`Clipboard`] over through [`portal::hand_over`]. This makes
//!   `input::start()` a prerequisite: call it before [`start`], as the daemon does.
//!
//! Both backends read on a copy (formats and size limits in [`mime`]), serve
//! pastes lazily ([`served`]) and drive their pipes without blocking the
//! compositor or D-Bus connection ([`transfer`], [`pipe`]).
//!
//! Not verified on a live compositor; see `docs/checklists/clipboard-wayland.md`.

mod data_control;
mod mime;
mod pipe;
pub(crate) mod portal;
mod served;
mod transfer;

use std::time::Duration;

use super::Clipboard;

const CHANNEL_CAPACITY: usize = 16;
/// Per format; a source that stalls longer is left out of the announcement.
const READ_TIMEOUT: Duration = Duration::from_secs(2);
/// Per paste, for the reader to drain what we write.
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a paste waits for the origin machine to send a lazy item.
const PASTE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_TRANSFERS: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum WaylandError {
    #[error("cannot connect to the Wayland compositor: {0}")]
    Connect(#[from] wayland_client::ConnectError),
    #[error("cannot read the Wayland globals: {0}")]
    Globals(#[from] wayland_client::globals::GlobalError),
    #[error("the compositor has no seat for data control")]
    NoSeat,
    #[error("Wayland connection failed: {0}")]
    Connection(String),
    #[error(
        "the compositor offers no data-control protocol and the clipboard portal was not granted"
    )]
    NoClipboardAccess,
    #[error("cannot start a clipboard thread: {0}")]
    Thread(std::io::Error),
}

/// Data-control first, then the portal access the input backend obtained.
pub(super) fn start() -> Result<Clipboard, WaylandError> {
    match data_control::start() {
        Err(data_control::StartError::Missing) => {}
        Err(data_control::StartError::Failed(error)) => {
            return portal::take().ok_or(error);
        }
        Ok(clipboard) => return Ok(clipboard),
    }
    portal::take().ok_or(WaylandError::NoClipboardAccess)
}
