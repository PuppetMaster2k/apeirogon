// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_proto::limits;
use interprocess::{
    local_socket::{GenericNamespaced, ListenerOptions, tokio::prelude::*},
    os::windows::{local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor},
};
use widestring::U16CString;

use super::{Listener, SocketError, SocketLocation, Stream};

pub(super) fn default_location() -> Result<SocketLocation, SocketError> {
    let user = std::env::var("USERNAME")
        .ok()
        .filter(|user| !user.is_empty())
        .ok_or(SocketError::NoUserLocation)?;
    Ok(SocketLocation::new(format!("apeirogon-{user}")))
}

#[allow(
    clippy::unused_async,
    reason = "matches the Unix backend's async signature"
)]
pub(super) async fn bind_user_only(location: &SocketLocation) -> Result<Listener, SocketError> {
    // OW grants rights to the object's owner, with no inherited allow entries.
    let sddl =
        U16CString::from_str("D:P(A;;GA;;;OW)").expect("the static SDDL contains no interior NUL");
    let descriptor = SecurityDescriptor::deserialize(&sddl)?;
    let name = location
        .as_path()
        .as_os_str()
        .to_ns_name::<GenericNamespaced>()?;
    Ok(ListenerOptions::new()
        .name(name)
        .security_descriptor(descriptor)
        .create_tokio()?)
}

pub(super) async fn connect(location: &SocketLocation) -> Result<Stream, SocketError> {
    let name = location
        .as_path()
        .as_os_str()
        .to_ns_name::<GenericNamespaced>()?;
    tokio::time::timeout(limits::HANDSHAKE_TIMEOUT, Stream::connect(name))
        .await
        .map_err(|_| SocketError::Timeout)?
        .map_err(Into::into)
}
