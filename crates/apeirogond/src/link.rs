// SPDX-License-Identifier: GPL-3.0-or-later

//! An authenticated control link to one peer, and the registry that settles
//! simultaneous dials (PROTOCOL.md 4).

use std::collections::HashMap;

use a7n_proto::v1::{Capability, PeerInfo};
use a7n_proto::{MachineId, ProtoError};
use quinn::{Connection, ConnectionError, RecvStream, SendStream, VarInt};
use rustls::pki_types::CertificateDer;

use crate::identity::{self, IdentityError};

/// Application close code for the link that lost a simultaneous dial.
pub(crate) const CLOSE_SUPERSEDED: VarInt = VarInt::from_u32(1);

/// Application close code for a link the peer stopped talking to.
pub(crate) const CLOSE_SILENT: VarInt = VarInt::from_u32(2);
/// Application close code for a link whose input went unacknowledged.
pub(crate) const CLOSE_UNACKED: VarInt = VarInt::from_u32(3);

/// Why a link could not be established or kept.
#[derive(Debug, thiserror::Error)]
pub(crate) enum LinkError {
    #[error("cannot start connecting: {0}")]
    Connect(#[from] quinn::ConnectError),
    #[error("connection failed: {0}")]
    Connection(#[from] ConnectionError),
    #[error("cannot read the control stream: {0}")]
    Read(#[from] quinn::ReadError),
    #[error("cannot write the control stream: {0}")]
    Write(#[from] quinn::WriteError),
    #[error("handshake timed out")]
    Timeout,
    #[error("the control stream ended early")]
    StreamClosed,
    #[error(transparent)]
    Proto(#[from] ProtoError),
    #[error("update {name} to protocol v{version}")]
    UpdateRequired { name: String, version: u32 },
    #[error("the daemon loop stopped receiving from this link")]
    InboundClosed,
    #[error("peer sent an invalid handshake message")]
    InvalidHello,
    #[error("peer presented no certificate")]
    NoPeerCertificate,
    #[error(transparent)]
    Identity(#[from] IdentityError),
}

/// Who opened the QUIC connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    Initiator,
    Responder,
}

/// An established link: the QUIC connection (datagrams, transfer streams),
/// control stream 0 and what the handshake learned.
#[derive(Debug)]
pub(crate) struct Link {
    pub(crate) connection: Connection,
    pub(crate) control_send: SendStream,
    pub(crate) control_recv: RecvStream,
    /// Bytes read from stream 0 after the handshake frame, not yet decoded.
    pub(crate) control_buf: Vec<u8>,
    pub(crate) peer_id: MachineId,
    pub(crate) peer_info: PeerInfo,
    pub(crate) capabilities: Vec<Capability>,
    pub(crate) role: Role,
}

/// Machine id of the certificate the peer authenticated with.
pub(crate) fn peer_machine_id(connection: &Connection) -> Result<MachineId, LinkError> {
    let identity = connection
        .peer_identity()
        .ok_or(LinkError::NoPeerCertificate)?;
    let chain = identity
        .downcast::<Vec<CertificateDer<'static>>>()
        .map_err(|_| LinkError::NoPeerCertificate)?;
    let leaf = chain.first().ok_or(LinkError::NoPeerCertificate)?;
    Ok(identity::machine_id_of(leaf)?)
}

/// The initiator that wins a simultaneous dial: the larger machine id.
pub(crate) fn surviving_initiator(a: MachineId, b: MachineId) -> MachineId {
    a.max(b)
}

/// Whether a new link replaces the one already held for the same peer.
///
/// It loses only if the old link was initiated by the surviving (larger) id
/// and the new one was not. Otherwise it is a reconnect or the winner, and
/// the old link is stale.
fn new_link_wins(own: MachineId, peer: MachineId, new: Role, existing: Role) -> bool {
    let survivor = surviving_initiator(own, peer);
    let initiator = |role| if role == Role::Initiator { own } else { peer };
    initiator(new) == survivor || initiator(existing) != survivor
}

/// Live links by peer machine id; owned by the task that routes between them.
#[derive(Debug, Default)]
pub(crate) struct Links {
    by_peer: HashMap<MachineId, Link>,
}

impl Links {
    /// Stores `link` and returns the link that must now be closed with
    /// [`CLOSE_SUPERSEDED`]: the loser of a simultaneous dial or a stale link.
    pub(crate) fn insert(&mut self, link: Link, own_id: MachineId) -> Option<Link> {
        let peer = link.peer_id;
        let Some(existing) = self.by_peer.get(&peer) else {
            self.by_peer.insert(peer, link);
            return None;
        };
        if new_link_wins(own_id, peer, link.role, existing.role) {
            return self.by_peer.insert(peer, link);
        }
        Some(link)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(byte: u8) -> MachineId {
        MachineId::try_from(&[byte; MachineId::LEN][..]).unwrap()
    }

    #[test]
    fn new_link_wins_simultaneous_dial_keeps_larger_initiator() {
        // Arrange: (own, peer, new role, existing role, new link survives)
        let cases = [
            (id(2), id(1), Role::Initiator, Role::Responder, true),
            (id(2), id(1), Role::Responder, Role::Initiator, false),
            (id(1), id(2), Role::Responder, Role::Initiator, true),
            (id(1), id(2), Role::Initiator, Role::Responder, false),
        ];

        for (own, peer, new, existing, expected) in cases {
            // Act
            let wins = new_link_wins(own, peer, new, existing);

            // Assert
            assert_eq!(wins, expected, "{own:?} {peer:?} {new:?} {existing:?}");
        }
    }
}
