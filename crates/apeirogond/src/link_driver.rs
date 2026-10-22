// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs one established link: liveness pings, acks, and hands every other
//! message to the daemon loop.

use a7n_core::liveness::{self, DropReason, Liveness, Poll};
use a7n_proto::v1::envelope::Body;
use a7n_proto::v1::{Ack, Envelope};
use a7n_proto::{FrameKind, MachineId, decode_datagram, limits};
use quinn::VarInt;
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep_until};

use crate::handshake::{read_frame, send_envelope};
use crate::link::{CLOSE_SILENT, CLOSE_UNACKED, Link, LinkError, Role};

/// `Ack.held_us` while nothing is injected. Once input injection lands (#12)
/// this becomes the time between dealing with a batch and sending its Ack.
const HELD_US_NOT_MEASURED: u32 = 0;

/// What woke the loop.
enum Event {
    Frame(Envelope),
    Datagram(Envelope),
    Timer,
    Outbound(Option<Envelope>),
}

/// Drives `link` until liveness drops it (`Ok`) or it fails (`Err`).
///
/// One task owns the link and its [`Liveness`]; received messages other than
/// `Ping` and `Ack` go to `inbound` tagged with the peer.
///
/// All `select!` branches are cancel-safe: `read_frame` keeps partial
/// frames in `link.control_buf`, `read_datagram` and `sleep_until` hold no
/// state. Mailbox receive is cancel-safe; writes complete outside `select!`.
pub(crate) async fn drive(
    mut link: Link,
    inbound: mpsc::Sender<(MachineId, usize, Envelope)>,
    mut outbound: mpsc::Receiver<Envelope>,
) -> Result<DropReason, LinkError> {
    let mut liveness = Liveness::new(core_role(link.role), std::time::Instant::now());
    let mut send_buf = Vec::new();
    // When the partial frame in `control_buf` started; a `select!` wake-up
    // restarts `read_frame`'s own timer, so the 30 s bound is kept here.
    let mut partial_since: Option<std::time::Instant> = None;
    loop {
        let frame_deadline = partial_since.map(|since| since + limits::FRAME_READ_TIMEOUT);
        let deadline =
            Instant::from_std(frame_deadline.map_or(liveness.next_deadline(), |frame| {
                frame.min(liveness.next_deadline())
            }));
        let event = tokio::select! {
            frame = read_frame(&mut link.control_recv, &mut link.control_buf, FrameKind::Control) => Event::Frame(frame?),
            datagram = link.connection.read_datagram() => motion_event(&datagram?)?,
            envelope = outbound.recv() => Event::Outbound(envelope),
            () = sleep_until(deadline) => Event::Timer,
        };
        let now = std::time::Instant::now();
        if frame_deadline.is_some_and(|frame| now >= frame) {
            return Err(LinkError::Timeout);
        }
        match event {
            Event::Frame(envelope) => {
                liveness.on_received(now);
                handle_frame(
                    &mut link,
                    &mut liveness,
                    &mut send_buf,
                    &inbound,
                    envelope,
                    now,
                )
                .await?;
            }
            Event::Datagram(envelope) => {
                liveness.on_received(now);
                forward(&inbound, &link, envelope).await?;
            }
            Event::Outbound(Some(envelope)) => {
                if let Some(Body::Input(batch)) = &envelope.body {
                    liveness.on_input_sent(batch.seq, now);
                }
                if let Some(body) = envelope.body {
                    send_envelope(&mut link.control_send, &mut send_buf, body).await?;
                }
            }
            Event::Outbound(None) => return Err(LinkError::InboundClosed),
            Event::Timer => {}
        }
        partial_since = if link.control_buf.is_empty() {
            None
        } else {
            partial_since.or(Some(now))
        };
        match liveness.poll(now) {
            Poll::Idle => {}
            Poll::SendPing(ping) => {
                send_envelope(&mut link.control_send, &mut send_buf, Body::Ping(ping)).await?;
            }
            Poll::Drop(reason) => {
                link.connection.close(close_code(reason), b"");
                return Ok(reason);
            }
        }
    }
}

fn motion_event(datagram: &[u8]) -> Result<Event, LinkError> {
    // Datagrams other than Motion are ignored; Timer just re-polls.
    Ok(match decode_datagram(datagram)? {
        Some(motion) => Event::Datagram(Envelope {
            body: Some(Body::Motion(motion)),
        }),
        None => Event::Timer,
    })
}

async fn handle_frame(
    link: &mut Link,
    liveness: &mut Liveness,
    send_buf: &mut Vec<u8>,
    inbound: &mpsc::Sender<(MachineId, usize, Envelope)>,
    envelope: Envelope,
    now: std::time::Instant,
) -> Result<(), LinkError> {
    match &envelope.body {
        Some(Body::Ping(ping)) => {
            // The round-trip time is not consumed yet; status output will.
            let _rtt = liveness.on_ping(ping, now);
            if let Some(echo) = liveness::answer(ping) {
                send_envelope(&mut link.control_send, send_buf, Body::Ping(echo)).await?;
            }
            Ok(())
        }
        Some(Body::Ack(ack)) => {
            let _rtt = liveness.on_ack(ack.seq, ack.held_us, now);
            Ok(())
        }
        Some(Body::Input(batch)) => {
            let ack = Ack {
                seq: batch.seq,
                held_us: HELD_US_NOT_MEASURED,
                locked: false,
            };
            send_envelope(&mut link.control_send, send_buf, Body::Ack(ack)).await?;
            forward(inbound, link, envelope).await
        }
        _ => forward(inbound, link, envelope).await,
    }
}

async fn forward(
    inbound: &mpsc::Sender<(MachineId, usize, Envelope)>,
    link: &Link,
    envelope: Envelope,
) -> Result<(), LinkError> {
    inbound
        .send((link.peer_id, link.connection.stable_id(), envelope))
        .await
        .map_err(|_| LinkError::InboundClosed)
}

fn close_code(reason: DropReason) -> VarInt {
    match reason {
        DropReason::Silence => CLOSE_SILENT,
        DropReason::UnackedInput => CLOSE_UNACKED,
    }
}

fn core_role(role: Role) -> liveness::Role {
    match role {
        Role::Initiator => liveness::Role::Initiator,
        Role::Responder => liveness::Role::Responder,
    }
}
