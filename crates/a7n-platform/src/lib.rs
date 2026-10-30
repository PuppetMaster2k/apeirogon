// SPDX-License-Identifier: GPL-3.0-or-later

pub mod autostart;
pub mod clipboard;
pub mod fs;
pub mod input;
pub mod local_socket;
pub mod permissions;

/// OS host name, bounded to the peer name limit, if available and nonempty.
#[must_use]
pub fn host_name() -> Option<String> {
    let name = os_host_name()?;
    let name: String = name
        .chars()
        .take(a7n_proto::limits::PEER_NAME_MAX)
        .collect();
    (!name.is_empty()).then_some(name)
}

#[cfg(unix)]
fn os_host_name() -> Option<String> {
    rustix::system::uname()
        .nodename()
        .to_str()
        .ok()
        .map(str::to_owned)
}

#[cfg(windows)]
fn os_host_name() -> Option<String> {
    std::env::var("COMPUTERNAME").ok()
}

#[cfg(feature = "tray")]
pub mod tray;
