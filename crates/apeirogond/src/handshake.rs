// SPDX-License-Identifier: GPL-3.0-or-later

//! `Hello`/`Welcome` on control stream 0 (PROTOCOL.md 4).

use a7n_proto::v1::envelope::Body;
use a7n_proto::v1::{Capability, Envelope, HandshakeError, Hello, PeerInfo, Welcome};
use a7n_proto::{FrameKind, PROTOCOL_VERSION, decode_frame, encode_frame, limits};
use quinn::{Connection, RecvStream, SendStream};
use tokio::time::timeout;

use crate::link::{Link, LinkError, Role, peer_machine_id};

/// The two sides run different protocol major versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VersionMismatch {
    pub(crate) local: u32,
    pub(crate) remote: u32,
}

/// Major versions must be equal.
pub(crate) fn check_version(local: u32, remote: u32) -> Result<(), VersionMismatch> {
    if local == remote {
        return Ok(());
    }
    Err(VersionMismatch { local, remote })
}

/// Turns a mismatch into the user-facing error that names the machine with
/// the older version.
pub(crate) fn version_error(
    mismatch: VersionMismatch,
    local_name: &str,
    remote_name: &str,
) -> LinkError {
    if mismatch.local < mismatch.remote {
        return LinkError::UpdateRequired {
            name: local_name.to_owned(),
            version: mismatch.remote,
        };
    }
    LinkError::UpdateRequired {
        name: remote_name.to_owned(),
        version: mismatch.local,
    }
}

/// Capabilities both sides list. Values this build does not know (from a
/// newer peer) are not in the intersection.
pub(crate) fn common_capabilities(local: &[i32], remote: &[i32]) -> Vec<Capability> {
    local
        .iter()
        .filter(|value| remote.contains(value))
        .filter_map(|&value| Capability::try_from(value).ok())
        .filter(|&capability| capability != Capability::Unspecified)
        .collect()
}

async fn send_message(send: &mut SendStream, body: Body) -> Result<(), LinkError> {
    let mut buf = Vec::new();
    encode_frame(&Envelope { body: Some(body) }, &mut buf);
    send.write_all(&buf).await?;
    Ok(())
}

/// Reads one frame of `kind` from `recv`, keeping surplus bytes in `buf`.
///
/// The frame must finish within `FRAME_READ_TIMEOUT` once its first byte
/// arrived (PROTOCOL.md 2).
pub(crate) async fn read_frame(
    recv: &mut RecvStream,
    buf: &mut Vec<u8>,
    kind: FrameKind,
) -> Result<Envelope, LinkError> {
    loop {
        if let Some((envelope, used)) = decode_frame::<Envelope>(buf, kind)? {
            buf.drain(..used);
            return Ok(envelope);
        }
        let read = recv.read_chunk(usize::MAX, true);
        let chunk = if buf.is_empty() {
            read.await?
        } else {
            timeout(limits::FRAME_READ_TIMEOUT, read)
                .await
                .map_err(|_| LinkError::Timeout)??
        };
        let chunk = chunk.ok_or(LinkError::StreamClosed)?;
        buf.extend_from_slice(&chunk.bytes);
    }
}

fn into_link(
    connection: Connection,
    streams: (SendStream, RecvStream),
    buf: Vec<u8>,
    info: PeerInfo,
    local: &PeerInfo,
    role: Role,
) -> Result<Link, LinkError> {
    Ok(Link {
        peer_id: peer_machine_id(&connection)?,
        capabilities: common_capabilities(&local.capabilities, &info.capabilities),
        connection,
        control_send: streams.0,
        control_recv: streams.1,
        control_buf: buf,
        peer_info: info,
        role,
    })
}

/// Initiator side: opens stream 0, sends `Hello`, reads `Welcome`.
pub(crate) async fn initiate(connection: Connection, local: &PeerInfo) -> Result<Link, LinkError> {
    let (mut send, mut recv) = connection.open_bi().await?;
    let hello = Hello {
        protocol_version: PROTOCOL_VERSION,
        info: Some(local.clone()),
    };
    send_message(&mut send, Body::Hello(hello)).await?;
    let mut buf = Vec::new();
    let Some(Body::Welcome(welcome)) = read_frame(&mut recv, &mut buf, FrameKind::Control)
        .await?
        .body
    else {
        return Err(LinkError::InvalidHello);
    };
    let info = welcome.info.clone().unwrap_or_default();
    if let Err(mismatch) = check_version(PROTOCOL_VERSION, welcome.protocol_version) {
        return Err(version_error(mismatch, &local.name, &info.name));
    }
    if welcome.error() != HandshakeError::None {
        return Err(LinkError::InvalidHello);
    }
    into_link(connection, (send, recv), buf, info, local, Role::Initiator)
}

/// Responder side: accepts stream 0, reads `Hello`, answers `Welcome`.
///
/// `accepts_drive` says whether this initiator may drive us.
pub(crate) async fn respond(
    connection: Connection,
    local: &PeerInfo,
    accepts_drive: bool,
) -> Result<Link, LinkError> {
    let (mut send, mut recv) = connection.accept_bi().await?;
    let mut buf = Vec::new();
    let first = read_frame(&mut recv, &mut buf, FrameKind::FirstHello).await?;
    let Some(Body::Hello(hello)) = first.body else {
        return reject(
            &mut send,
            local,
            HandshakeError::InvalidHello,
            LinkError::InvalidHello,
        )
        .await;
    };
    let info = hello.info.clone().unwrap_or_default();
    if let Err(mismatch) = check_version(PROTOCOL_VERSION, hello.protocol_version) {
        let error = version_error(mismatch, &local.name, &info.name);
        return reject(&mut send, local, HandshakeError::UnsupportedVersion, error).await;
    }
    let welcome = Welcome {
        protocol_version: PROTOCOL_VERSION,
        info: Some(local.clone()),
        accepts_drive,
        error: HandshakeError::None.into(),
    };
    send_message(&mut send, Body::Welcome(welcome)).await?;
    into_link(connection, (send, recv), buf, info, local, Role::Responder)
}

/// Sends a `Welcome` carrying `code`, waits until the peer has it, and
/// returns `error`.
async fn reject(
    send: &mut SendStream,
    local: &PeerInfo,
    code: HandshakeError,
    error: LinkError,
) -> Result<Link, LinkError> {
    let welcome = Welcome {
        protocol_version: PROTOCOL_VERSION,
        info: Some(local.clone()),
        accepts_drive: false,
        error: code.into(),
    };
    send_message(send, Body::Welcome(welcome)).await?;
    // Best effort: without this the close could overtake the Welcome.
    let _ = send.finish();
    let _ = send.stopped().await;
    Err(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_error_newer_peer_names_local_machine_to_update() {
        // Arrange
        let mismatch = check_version(1, 2).unwrap_err();

        // Act
        let error = version_error(mismatch, "laptop", "desktop");

        // Assert
        assert_eq!(error.to_string(), "update laptop to protocol v2");
    }

    #[test]
    fn version_error_older_peer_names_remote_machine_to_update() {
        // Arrange
        let mismatch = check_version(2, 1).unwrap_err();

        // Act
        let error = version_error(mismatch, "laptop", "desktop");

        // Assert
        assert_eq!(error.to_string(), "update desktop to protocol v2");
    }

    #[test]
    fn check_version_equal_major_is_accepted() {
        // Arrange / Act
        let result = check_version(1, 1);

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    fn common_capabilities_unknown_remote_capability_is_dropped() {
        // Arrange
        let unknown = 9999;
        let local = [
            Capability::ClipboardText as i32,
            Capability::Gestures as i32,
        ];
        let remote = [Capability::ClipboardText as i32, unknown];

        // Act
        let common = common_capabilities(&local, &remote);

        // Assert
        assert_eq!(common, vec![Capability::ClipboardText]);
    }
}
