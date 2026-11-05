// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs one established link: liveness pings, acks, and hands every other
//! message to the daemon loop.

use a7n_core::liveness::{self, DropReason, Liveness, Poll};
use a7n_proto::v1::envelope::Body;
use a7n_proto::v1::{Ack, Envelope};
use a7n_proto::{FrameKind, MachineId, decode_datagram, encode_datagram, limits};
use quinn::VarInt;
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep_until};

use crate::handshake::{read_frame, send_envelope};
use crate::link::{CLOSE_SILENT, CLOSE_UNACKED, Link, LinkError, Role};

/// `Ack.held_us` until the time between dealing with a batch and sending its
/// Ack is measured.
const HELD_US_NOT_MEASURED: u32 = 0;

/// What woke the loop.
enum Event {
    Frame(Envelope),
    Datagram(Envelope),
    Timer,
    Transfer(quinn::RecvStream),
    Outbound(Option<Envelope>),
}

/// Drives `link` until liveness drops it (`Ok`) or it fails (`Err`).
///
/// One task owns the link and its [`Liveness`]; received messages other than
/// `Ping` go to `inbound` tagged with the peer.
///
/// All `select!` branches are cancel-safe: `read_frame` keeps partial
/// frames in `link.control_buf`, `read_datagram` and `sleep_until` hold no
/// state. Mailbox receive is cancel-safe; writes complete outside `select!`.
pub(crate) async fn drive(
    link: Link,
    inbound: mpsc::Sender<(MachineId, usize, Envelope)>,
    mut outbound: mpsc::Receiver<Envelope>,
    transfers: mpsc::Sender<(MachineId, usize, quinn::RecvStream)>,
) -> Result<DropReason, LinkError> {
    let mut driver = Driver {
        liveness: Liveness::new(core_role(link.role), std::time::Instant::now()),
        link,
        send_buf: Vec::new(),
        input_seq: 0,
        partial_since: None,
    };
    loop {
        let (event, now) = driver.next_event(&mut outbound).await?;
        driver
            .handle_event(event, &inbound, &transfers, now)
            .await?;
        if let Some(reason) = driver.poll_liveness(now).await? {
            return Ok(reason);
        }
    }
}

struct Driver {
    link: Link,
    liveness: Liveness,
    send_buf: Vec<u8>,
    input_seq: u64,
    // A select wake-up restarts read_frame's timer, so keep the partial-frame bound here.
    partial_since: Option<std::time::Instant>,
}

impl Driver {
    async fn next_event(
        &mut self,
        outbound: &mut mpsc::Receiver<Envelope>,
    ) -> Result<(Event, std::time::Instant), LinkError> {
        let frame_deadline = self
            .partial_since
            .map(|since| since + limits::FRAME_READ_TIMEOUT);
        let deadline = Instant::from_std(
            frame_deadline.map_or(self.liveness.next_deadline(), |frame| {
                frame.min(self.liveness.next_deadline())
            }),
        );
        let event = tokio::select! {
            stream = self.link.connection.accept_uni() => Event::Transfer(stream?),
            frame = read_frame(&mut self.link.control_recv, &mut self.link.control_buf, FrameKind::Control) => Event::Frame(frame?),
            datagram = self.link.connection.read_datagram() => motion_event(&datagram?)?,
            envelope = outbound.recv() => Event::Outbound(envelope),
            () = sleep_until(deadline) => Event::Timer,
        };
        let now = std::time::Instant::now();
        if frame_deadline.is_some_and(|frame| now >= frame) {
            return Err(LinkError::Timeout);
        }
        Ok((event, now))
    }

    async fn handle_event(
        &mut self,
        event: Event,
        inbound: &mpsc::Sender<(MachineId, usize, Envelope)>,
        transfers: &mpsc::Sender<(MachineId, usize, quinn::RecvStream)>,
        now: std::time::Instant,
    ) -> Result<(), LinkError> {
        match event {
            Event::Frame(envelope) => {
                self.liveness.on_received(now);
                handle_frame(
                    &mut self.link,
                    &mut self.liveness,
                    &mut self.send_buf,
                    inbound,
                    envelope,
                    now,
                )
                .await?;
            }
            Event::Datagram(envelope) => {
                self.liveness.on_received(now);
                forward(inbound, &self.link, envelope).await?;
            }
            Event::Outbound(Some(envelope)) => {
                self.send_outbound(envelope, now).await?;
            }
            Event::Outbound(None) => return Err(LinkError::InboundClosed),
            Event::Transfer(stream) => {
                transfers
                    .send((self.link.peer_id, self.link.connection.stable_id(), stream))
                    .await
                    .map_err(|_| LinkError::InboundClosed)?;
            }
            Event::Timer => {}
        }
        self.partial_since = if self.link.control_buf.is_empty() {
            None
        } else {
            self.partial_since.or(Some(now))
        };
        Ok(())
    }

    async fn send_outbound(
        &mut self,
        mut envelope: Envelope,
        now: std::time::Instant,
    ) -> Result<(), LinkError> {
        if matches!(envelope.body, Some(Body::Motion(_))) {
            self.send_buf.clear();
            encode_datagram(&envelope, &mut self.send_buf);
            // ponytail: one small allocation per motion datagram; pool buffers if profiling shows it.
            if let Err(error) = self
                .link
                .connection
                .send_datagram(self.send_buf.clone().into())
            {
                match error {
                    quinn::SendDatagramError::ConnectionLost(error) => {
                        return Err(error.into());
                    }
                    _ => crate::log::event!("kind=motion_dropped"),
                }
            }
        } else {
            if let Some(Body::Input(batch)) = &mut envelope.body {
                self.input_seq = self
                    .input_seq
                    .checked_add(1)
                    .ok_or(LinkError::InputSeqExhausted)?;
                batch.seq = self.input_seq;
                self.liveness.on_input_sent(batch.seq, now);
            }
            if let Some(body) = envelope.body {
                send_envelope(&mut self.link.control_send, &mut self.send_buf, body).await?;
            }
        }
        Ok(())
    }

    async fn poll_liveness(
        &mut self,
        now: std::time::Instant,
    ) -> Result<Option<DropReason>, LinkError> {
        match self.liveness.poll(now) {
            Poll::Idle => {}
            Poll::SendPing(ping) => {
                send_envelope(
                    &mut self.link.control_send,
                    &mut self.send_buf,
                    Body::Ping(ping),
                )
                .await?;
            }
            Poll::Drop(reason) => {
                self.link.connection.close(close_code(reason), b"");
                return Ok(Some(reason));
            }
        }
        Ok(None)
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
            forward(inbound, link, envelope).await
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

#[cfg(test)]
mod tests;
