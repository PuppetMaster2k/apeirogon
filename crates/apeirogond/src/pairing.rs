// SPDX-License-Identifier: GPL-3.0-or-later

//! Pairing over a separate mutually authenticated QUIC connection (PROTOCOL.md 5).

use std::future::Future;
use std::time::Instant;

use a7n_core::pairing::{HostPairing, Paired, RequesterPairing};
use a7n_proto::v1::{
    PairAnswer, PairConfirm, PairDone, PairError, PairingMessage, PeerInfo, pairing_message::Body,
};
use a7n_proto::{FrameKind, Pin, encode_frame, limits};
use quinn::{Connection, RecvStream, SendStream};
use rand_core::{CryptoRng, RngCore};
use tokio::time::timeout;

use crate::identity;
use crate::link::{LinkError, peer_certificate};
use crate::pin_store::Entry;

/// Domain separation for the SPAKE2 additional authenticated data.
const BINDING_LABEL: &[u8] = b"a7n-pair/1 binding";

/// Details stay local; the wire always carries a refusal without an explanation.
#[derive(Debug, thiserror::Error)]
pub(crate) enum PairingError {
    #[error("pairing refused")]
    Refused,
    #[error(transparent)]
    Link(#[from] LinkError),
}

/// Dials pairing TLS within the handshake deadline; configure the endpoint with
/// `tls::pair_client_config` first.
pub(crate) async fn dial(
    endpoint: &quinn::Endpoint,
    addr: std::net::SocketAddr,
) -> Result<Connection, PairingError> {
    timeout(limits::HANDSHAKE_TIMEOUT, async {
        endpoint
            .connect(addr, crate::tls::TLS_SERVER_NAME)
            .map_err(LinkError::from)?
            .await
            .map_err(LinkError::from)
    })
    .await
    .map_err(|_| LinkError::Timeout)?
    .map_err(Into::into)
}

fn peer_pin(connection: &Connection) -> Result<Pin, PairingError> {
    identity::pin_of(&peer_certificate(connection)?)
        .map_err(LinkError::from)
        .map_err(Into::into)
}

fn binding(connection: &Connection) -> Result<[u8; 32], PairingError> {
    let mut ctx = [0; 32];
    connection
        .export_keying_material(&mut ctx, BINDING_LABEL, b"")
        .map_err(|_| PairingError::Refused)?;
    Ok(ctx)
}

pub(super) async fn write_message(send: &mut SendStream, body: Body) -> Result<(), PairingError> {
    let mut buf = Vec::new();
    encode_frame(&PairingMessage { body: Some(body) }, &mut buf);
    timeout(limits::HANDSHAKE_TIMEOUT, send.write_all(&buf))
        .await
        .map_err(|_| LinkError::Timeout)?
        .map_err(LinkError::from)?;
    Ok(())
}

pub(super) async fn read_message(
    recv: &mut RecvStream,
    buf: &mut Vec<u8>,
) -> Result<Body, PairingError> {
    timeout(limits::HANDSHAKE_TIMEOUT, async {
        let message: PairingMessage = crate::frame::read(
            recv,
            buf,
            FrameKind::Pairing,
            crate::frame::ChunkBound::Fixed(limits::CONTROL_FRAME_MAX),
            None,
        )
        .await?
        .ok_or(LinkError::StreamClosed)?;
        message.body.ok_or(PairingError::Refused)
    })
    .await
    .map_err(|_| LinkError::Timeout)?
}

async fn finish(send: &mut SendStream) -> Result<(), PairingError> {
    send.finish().map_err(|_| PairingError::Refused)?;
    // Delivery must precede connection close, particularly for PairDone.
    timeout(limits::HANDSHAKE_TIMEOUT, send.stopped())
        .await
        .map_err(|_| LinkError::Timeout)?
        .map_err(|_| PairingError::Refused)?;
    Ok(())
}

/// Runs a host exchange in a task. Only the owner may persist a confirmed peer;
/// its reply supplies `PairDone` after admission succeeds.
pub(crate) async fn respond<F, Fut>(
    connection: Connection,
    mut host: HostPairing,
    info: PeerInfo,
    admit: F,
    mut now: impl FnMut() -> Instant,
    rng: &mut (impl CryptoRng + RngCore),
) -> Result<(), PairingError>
where
    F: FnOnce(Entry, Paired) -> Fut,
    Fut: Future<Output = Result<PairDone, PairingError>>,
{
    let streams = timeout(limits::HANDSHAKE_TIMEOUT, connection.accept_bi()).await;
    let Ok(Ok((mut send, mut recv))) = streams else {
        host.refuse();
        connection.close(crate::link::CLOSE_NORMAL, b"");
        return Err(PairingError::Refused);
    };
    let mut failure = Body::Answer(PairAnswer {
        error: PairError::Refused.into(),
        ..PairAnswer::default()
    });
    let result = async {
        let pin = peer_pin(&connection)?;
        let ctx = binding(&connection)?;
        let mut buf = Vec::new();
        let Body::Start(start) = read_message(&mut recv, &mut buf).await? else {
            return Err(PairingError::Refused);
        };
        let answer = host
            .start(start, pin, ctx, info.clone(), now(), rng)
            .map_err(|_| PairingError::Refused)?;
        write_message(&mut send, Body::Answer(answer)).await?;
        failure = Body::Done(PairDone {
            error: PairError::Refused.into(),
            ..PairDone::default()
        });
        let Body::Confirm(confirm) = read_message(&mut recv, &mut buf).await? else {
            return Err(PairingError::Refused);
        };
        let paired = host
            .confirm(confirm, now())
            .map_err(|_| PairingError::Refused)?;
        let entry = peer_entry(&connection, paired.peer.pin, paired.peer.info.name.clone())?;
        let done = timeout(limits::HANDSHAKE_TIMEOUT, admit(entry, paired))
            .await
            .map_err(|_| LinkError::Timeout)??;
        write_message(&mut send, Body::Done(done)).await?;
        finish(&mut send).await?;
        Ok(())
    }
    .await;
    if result.is_err() {
        host.refuse();
        refuse(&mut send, failure).await;
    }
    connection.close(crate::link::CLOSE_NORMAL, b"");
    result
}

/// Returns a peer for owner-only persistence after the host's storage proof.
pub(crate) async fn initiate(
    connection: Connection,
    mut requester: RequesterPairing,
) -> Result<Entry, PairingError> {
    let streams = timeout(limits::HANDSHAKE_TIMEOUT, connection.open_bi()).await;
    let Ok(Ok((mut send, mut recv))) = streams else {
        connection.close(crate::link::CLOSE_NORMAL, b"");
        return Err(PairingError::Refused);
    };
    let result = async {
        let pin = peer_pin(&connection)?;
        let ctx = binding(&connection)?;
        write_message(&mut send, Body::Start(requester.start())).await?;
        let mut buf = Vec::new();
        let Body::Answer(answer) = read_message(&mut recv, &mut buf).await? else {
            return Err(PairingError::Refused);
        };
        let confirm = requester
            .answer(answer, pin, ctx)
            .map_err(|_| PairingError::Refused)?;
        write_message(&mut send, Body::Confirm(confirm)).await?;
        let Body::Done(done) = read_message(&mut recv, &mut buf).await? else {
            return Err(PairingError::Refused);
        };
        let peer = requester.done(done).map_err(|_| PairingError::Refused)?;
        peer_entry(&connection, peer.pin, peer.info.name)
    }
    .await;
    if result.is_err() {
        refuse(
            &mut send,
            Body::Confirm(PairConfirm {
                error: PairError::Refused.into(),
                ..PairConfirm::default()
            }),
        )
        .await;
    }
    connection.close(crate::link::CLOSE_NORMAL, b"");
    result
}

async fn refuse(send: &mut SendStream, body: Body) {
    if write_message(send, body).await.is_ok() {
        let _ = finish(send).await;
    }
}

// The name reaches terminals, notifications and pins.toml, so refuse escapes
// as well as names outside the wire bound.
fn valid_peer_name(name: &str) -> bool {
    (1..=limits::PEER_NAME_MAX).contains(&name.chars().count())
        && !name.chars().any(char::is_control)
}

fn peer_entry(connection: &Connection, pin: Pin, name: String) -> Result<Entry, PairingError> {
    if !valid_peer_name(&name) {
        return Err(PairingError::Refused);
    }
    let cert = peer_certificate(connection)?;
    if identity::pin_of(&cert).map_err(LinkError::from)? != pin {
        return Err(PairingError::Refused);
    }
    Ok(Entry {
        machine_id: identity::machine_id_of(&cert).map_err(LinkError::from)?,
        pin,
        name,
        spki_der: Some(identity::spki_of(&cert).map_err(LinkError::from)?),
        hw_addr: None,
        screen_mm: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_peer_name_unsafe_names_are_rejected_and_plain_name_is_accepted() {
        // Arrange
        let long = "x".repeat(limits::PEER_NAME_MAX + 1);
        let cases = [
            ("", false),
            ("desk\u{1b}[2J", false),
            ("a\nb", false),
            ("tab\there", false),
            (long.as_str(), false),
            ("Büro-Mac ✓", true),
        ];

        for (name, expected) in cases {
            // Act
            let valid = valid_peer_name(name);

            // Assert
            assert_eq!(valid, expected, "{name:?}");
        }
    }
}
