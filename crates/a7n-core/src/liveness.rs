// SPDX-License-Identifier: GPL-3.0-or-later

//! Link liveness: ping cadence, round-trip time and the two drop rules
//! (PROTOCOL.md 9). A pure state machine; the daemon passes `Instant` values
//! in and acts on [`Poll`].

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use a7n_proto::limits;
use a7n_proto::v1::Ping;

/// Which side opened the link; the silence limit differs so that both sides
/// do not drop at the same instant (PROTOCOL.md 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Initiator,
    Responder,
}

impl Role {
    fn silence_drop(self) -> Duration {
        match self {
            Role::Initiator => limits::INITIATOR_SILENCE_DROP,
            Role::Responder => limits::RESPONDER_SILENCE_DROP,
        }
    }
}

/// Why the link must be closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    /// Nothing arrived from the peer for the role's silence limit.
    Silence,
    /// The oldest `InputBatch` stayed unacknowledged too long.
    UnackedInput,
}

/// What the driver must do now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Poll {
    Idle,
    SendPing(Ping),
    Drop(DropReason),
}

/// Liveness of one link.
///
/// Limits are inclusive: a link silent for exactly the limit is dropped.
#[derive(Debug)]
pub struct Liveness {
    role: Role,
    last_received: Instant,
    last_ping_sent: Instant,
    /// Never 0: in proto3 0 is "unset", and `echo == 0` must mean "no echo".
    next_probe: u32,
    /// Only the newest probe is timed; an echo of an older one is ignored.
    probe_in_flight: Option<(u32, Instant)>,
    unacked: VecDeque<(u64, Instant)>,
}

impl Liveness {
    #[must_use]
    pub fn new(role: Role, now: Instant) -> Self {
        Self {
            role,
            last_received: now,
            last_ping_sent: now,
            next_probe: 1,
            probe_in_flight: None,
            unacked: VecDeque::new(),
        }
    }

    /// Any frame or datagram arrived from the peer.
    pub fn on_received(&mut self, now: Instant) {
        self.last_received = now;
    }

    /// An `InputBatch` with `seq` was sent.
    pub fn on_input_sent(&mut self, seq: u64, now: Instant) {
        self.unacked.push_back((seq, now));
    }

    /// An `Ack` arrived. Acks are cumulative, so every batch up to `ack_seq`
    /// is cleared. Returns the round-trip time of the newest cleared batch:
    /// `now - sent_at - held_us`, never below zero.
    pub fn on_ack(&mut self, ack_seq: u64, held_us: u32, now: Instant) -> Option<Duration> {
        let mut newest_sent_at = None;
        while let Some(&(seq, sent_at)) = self.unacked.front() {
            if seq > ack_seq {
                break;
            }
            newest_sent_at = Some(sent_at);
            self.unacked.pop_front();
        }
        let held = Duration::from_micros(u64::from(held_us));
        newest_sent_at.map(|sent_at| now.saturating_duration_since(sent_at).saturating_sub(held))
    }

    /// A `Ping` arrived. Returns the round-trip time when it echoes the
    /// probe we are timing. A probe needs an answer from [`answer`].
    pub fn on_ping(&mut self, ping: &Ping, now: Instant) -> Option<Duration> {
        let (probe, sent_at) = self.probe_in_flight?;
        if ping.echo != probe {
            return None;
        }
        self.probe_in_flight = None;
        Some(now.saturating_duration_since(sent_at))
    }

    /// Decides what to do at `now`. Call it when [`Self::next_deadline`]
    /// passes and after every event.
    pub fn poll(&mut self, now: Instant) -> Poll {
        if now.saturating_duration_since(self.last_received) >= self.role.silence_drop() {
            return Poll::Drop(DropReason::Silence);
        }
        if let Some(&(_, sent_at)) = self.unacked.front()
            && now.saturating_duration_since(sent_at) >= limits::UNACKED_INPUT_DROP
        {
            return Poll::Drop(DropReason::UnackedInput);
        }
        if now.saturating_duration_since(self.last_ping_sent) < limits::PING_INTERVAL {
            return Poll::Idle;
        }
        self.last_ping_sent = now;
        Poll::SendPing(self.new_probe(now))
    }

    /// The earliest instant at which [`Self::poll`] may have work.
    #[must_use]
    pub fn next_deadline(&self) -> Instant {
        let silence = self.last_received + self.role.silence_drop();
        let ping = self.last_ping_sent + limits::PING_INTERVAL;
        let unacked = self
            .unacked
            .front()
            .map(|&(_, sent_at)| sent_at + limits::UNACKED_INPUT_DROP);
        [Some(silence), Some(ping), unacked]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(silence)
    }

    fn new_probe(&mut self, now: Instant) -> Ping {
        let probe = self.next_probe;
        // Skip 0 on wrap, see `next_probe`.
        self.next_probe = self.next_probe.checked_add(1).unwrap_or(1);
        self.probe_in_flight = Some((probe, now));
        Ping { probe, echo: 0 }
    }
}

/// The reply to a received `Ping`: an echo for a probe, nothing for an echo
/// (an echo is never echoed).
#[must_use]
pub fn answer(ping: &Ping) -> Option<Ping> {
    if ping.probe == 0 {
        return None;
    }
    Some(Ping {
        probe: 0,
        echo: ping.probe,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    #[test]
    fn poll_silence_reaches_role_limit_drops_link() {
        // Arrange: (role, silent for, dropped)
        let cases = [
            (Role::Initiator, 1999, false),
            (Role::Initiator, 2000, true),
            (Role::Responder, 2000, false),
            (Role::Responder, 2499, false),
            (Role::Responder, 2500, true),
        ];

        for (role, silent_ms, dropped) in cases {
            let start = Instant::now();
            let mut liveness = Liveness::new(role, start);
            // Keep the ping rule out of the way.
            liveness.last_ping_sent = start + ms(silent_ms);

            // Act
            let poll = liveness.poll(start + ms(silent_ms));

            // Assert
            assert_eq!(
                poll == Poll::Drop(DropReason::Silence),
                dropped,
                "{role:?} {silent_ms} ms"
            );
        }
    }

    #[test]
    fn poll_oldest_batch_unacked_for_1500ms_drops_link() {
        // Arrange
        let start = Instant::now();
        let mut liveness = Liveness::new(Role::Responder, start);
        liveness.on_input_sent(1, start);
        liveness.on_input_sent(2, start + ms(400));
        liveness.on_received(start + ms(1500));
        liveness.last_ping_sent = start + ms(1500);

        // Act
        let before = liveness.poll(start + ms(1499));
        let at_limit = liveness.poll(start + ms(1500));

        // Assert
        assert_eq!(before, Poll::Idle);
        assert_eq!(at_limit, Poll::Drop(DropReason::UnackedInput));
    }

    #[test]
    fn on_ack_cumulative_seq_clears_older_batches() {
        // Arrange
        let start = Instant::now();
        let mut liveness = Liveness::new(Role::Responder, start);
        for seq in 1..=4 {
            liveness.on_input_sent(seq, start);
        }
        liveness.on_ack(3, 0, start + ms(10));
        liveness.on_received(start + ms(1500));
        liveness.last_ping_sent = start + ms(1500);

        // Act: batch 4 is still unacked and now too old
        let with_batch_four = liveness.poll(start + ms(1500));
        liveness.on_ack(4, 0, start + ms(1500));
        let after_last_ack = liveness.poll(start + ms(1500));

        // Assert
        assert_eq!(with_batch_four, Poll::Drop(DropReason::UnackedInput));
        assert_eq!(after_last_ack, Poll::Idle);
    }

    #[test]
    fn on_ack_held_time_is_subtracted_from_rtt() {
        // Arrange
        let start = Instant::now();
        let mut liveness = Liveness::new(Role::Initiator, start);
        liveness.on_input_sent(1, start);

        // Act
        let rtt = liveness.on_ack(1, 4_000, start + ms(10));
        let held_longer_than_elapsed = {
            liveness.on_input_sent(2, start);
            liveness.on_ack(2, 50_000, start + ms(10))
        };

        // Assert
        assert_eq!(rtt, Some(ms(6)));
        assert_eq!(held_longer_than_elapsed, Some(Duration::ZERO));
    }

    #[test]
    fn poll_one_second_after_last_ping_sends_probe() {
        // Arrange
        let start = Instant::now();
        let mut liveness = Liveness::new(Role::Initiator, start);
        liveness.on_received(start + ms(1000));

        // Act
        let early = liveness.poll(start + ms(999));
        let due = liveness.poll(start + ms(1000));
        let echo = Ping { probe: 0, echo: 1 };
        let rtt = liveness.on_ping(&echo, start + ms(1030));

        // Assert
        assert_eq!(early, Poll::Idle);
        assert_eq!(due, Poll::SendPing(Ping { probe: 1, echo: 0 }));
        assert_eq!(rtt, Some(ms(30)));
    }

    #[test]
    fn new_probe_counter_wrapped_skips_zero() {
        // Arrange
        let start = Instant::now();
        let mut liveness = Liveness::new(Role::Initiator, start);
        liveness.next_probe = u32::MAX;

        // Act
        let last = liveness.new_probe(start);
        let wrapped = liveness.new_probe(start);

        // Assert
        assert_eq!(last.probe, u32::MAX);
        assert_eq!(wrapped.probe, 1);
    }

    #[test]
    fn answer_echo_is_never_echoed() {
        // Arrange
        let probe = Ping { probe: 7, echo: 0 };
        let echo = Ping { probe: 0, echo: 7 };

        // Act
        let reply_to_probe = answer(&probe);
        let reply_to_echo = answer(&echo);

        // Assert
        assert_eq!(reply_to_probe, Some(echo));
        assert_eq!(reply_to_echo, None);
    }
}
