// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Duration;

use super::*;

// Deterministic test-only stream; never used as production randomness.
struct TestRng(u64);
impl RngCore for TestRng {
    fn next_u32(&mut self) -> u32 {
        let bytes = self.next_u64().to_le_bytes();
        u32::from_le_bytes(bytes[..4].try_into().unwrap())
    }
    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for chunk in dest.chunks_mut(8) {
            chunk.copy_from_slice(&self.next_u64().to_le_bytes()[..chunk.len()]);
        }
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}
impl CryptoRng for TestRng {}

fn pin(byte: u8) -> Pin {
    Pin::try_from(&[byte; 32][..]).unwrap()
}
fn info(name: &str) -> PeerInfo {
    PeerInfo {
        name: name.into(),
        ..PeerInfo::default()
    }
}
fn code() -> PairingCode {
    PairingCode::parse("012345").unwrap()
}
fn host(now: Instant) -> HostPairing {
    HostPairing::new(code(), None, pin(2), now)
}
fn requester(code: PairingCode) -> RequesterPairing {
    RequesterPairing::new(code, None, pin(1), info("requester"), &mut TestRng(1))
}
fn answer(host: &mut HostPairing, requester: &RequesterPairing, now: Instant) -> PairAnswer {
    host.start(
        requester.start(),
        pin(1),
        [3; 32],
        info("host"),
        now,
        &mut TestRng(2),
    )
    .unwrap()
}

#[test]
fn pairing_correct_code_returns_both_pins_only_after_confirmation() {
    // Arrange
    let now = Instant::now();
    let mut host = host(now);
    let mut requester = requester(code());
    let answer = answer(&mut host, &requester, now);

    // Act
    let confirm = requester.answer(answer, pin(2), [3; 32]).unwrap();
    let paired = host.confirm(confirm, now).unwrap();
    let stored_requester = (paired.peer.pin, paired.peer.info.name.clone());
    let stored_host = requester.done(paired.done()).unwrap();

    // Assert
    assert_eq!(stored_requester, (pin(1), "requester".into()));
    assert_eq!(stored_host.pin, pin(2));
    assert_eq!(stored_host.info.name, "host");
}

#[test]
fn answer_wrong_code_refuses_and_ends_host_pairing() {
    // Arrange
    let now = Instant::now();
    let mut host = host(now);
    let mut requester = requester(PairingCode::parse("999999").unwrap());
    let answer = answer(&mut host, &requester, now);

    // Act
    let refused = requester.answer(answer, pin(2), [3; 32]);
    let ended = host.confirm(
        PairConfirm {
            error: PairError::Refused.into(),
            tag: vec![],
        },
        now,
    );
    let retry = host.start(
        requester.start(),
        pin(1),
        [3; 32],
        info("host"),
        now,
        &mut TestRng(2),
    );

    // Assert
    assert_eq!(refused, Err(PairError::Refused));
    assert!(matches!(ended, Err(PairError::Refused)));
    assert_eq!(retry, Err(PairError::NotPairing));
}

#[test]
fn start_expired_code_refuses_and_closes_window() {
    // Arrange
    let now = Instant::now();
    let mut host = host(now);
    let requester = requester(code());

    // Act
    let result = host.start(
        requester.start(),
        pin(1),
        [3; 32],
        info("host"),
        now + limits::PAIRING_CODE_DURATION,
        &mut TestRng(2),
    );

    // Assert
    assert_eq!(result, Err(PairError::Refused));
    assert!(host.closed);
}

#[test]
fn confirm_expired_window_refuses_requester_proof() {
    // Arrange
    let now = Instant::now();
    let mut host = host(now);
    let mut requester = requester(code());
    let answer = answer(&mut host, &requester, now);
    let confirm = requester.answer(answer, pin(2), [3; 32]).unwrap();

    // Act
    let result = host.confirm(confirm, now + limits::PAIRING_CODE_DURATION);

    // Assert
    assert!(matches!(result, Err(PairError::Refused)));
}

#[test]
fn answer_mismatched_identities_or_binding_refuses_confirmation() {
    // Arrange
    let now = Instant::now();
    for (requester_pin, host_pin, ctx) in [
        (pin(9), pin(2), [3; 32]),
        (pin(1), pin(9), [3; 32]),
        (pin(1), pin(2), [9; 32]),
    ] {
        let mut host = host(now);
        let mut requester = requester(code());
        let answer = host
            .start(
                requester.start(),
                requester_pin,
                [3; 32],
                info("host"),
                now,
                &mut TestRng(2),
            )
            .unwrap();

        // Act
        let result = requester.answer(answer, host_pin, ctx);

        // Assert
        assert_eq!(result, Err(PairError::Refused));
    }
}

#[test]
fn done_tampered_tag_refuses_host_storage_proof() {
    // Arrange
    let now = Instant::now();
    let mut host = host(now);
    let mut requester = requester(code());
    let answer = answer(&mut host, &requester, now);
    let confirm = requester.answer(answer, pin(2), [3; 32]).unwrap();
    let mut done = host.confirm(confirm, now).unwrap().done();
    done.tag[0] ^= 1;

    // Act
    let result = requester.done(done);

    // Assert
    assert!(matches!(result, Err(PairError::Refused)));
}

#[test]
fn start_malformed_share_refuses_and_rejects_second_attempt() {
    // Arrange
    let now = Instant::now();
    for share in [vec![], vec![0; 31], vec![0; 33], vec![0; 32], vec![255; 32]] {
        let mut host = host(now);
        let mut start = requester(code()).start();
        start.share = share;

        // Act
        let result = host.start(
            start.clone(),
            pin(1),
            [3; 32],
            info("host"),
            now,
            &mut TestRng(2),
        );
        let retry = host.start(start, pin(1), [3; 32], info("host"), now, &mut TestRng(2));

        // Assert
        assert_eq!(result, Err(PairError::Refused));
        assert_eq!(retry, Err(PairError::NotPairing));
    }
}

#[test]
fn confirm_malformed_or_bad_tag_refuses_and_closes_window() {
    // Arrange
    let now = Instant::now();
    for tag in [vec![], vec![0; 31], vec![0; 32], vec![0; 33]] {
        let mut host = host(now);
        answer(&mut host, &requester(code()), now);

        // Act
        let result = host.confirm(
            PairConfirm {
                tag,
                error: PairError::None.into(),
            },
            now,
        );

        // Assert
        assert!(matches!(result, Err(PairError::Refused)));
        assert!(host.closed);
        assert!(host.pending.is_none());
    }
}

#[test]
fn pairing_qr_secret_match_pairs_and_mismatch_refuses() {
    // Arrange
    let now = Instant::now();
    for (requester_qr, succeeds) in [(Some([4; 16]), true), (Some([5; 16]), false), (None, false)] {
        let mut host = HostPairing::new(code(), Some([4; 16]), pin(2), now);
        let mut requester = RequesterPairing::new(
            code(),
            requester_qr,
            pin(1),
            info("requester"),
            &mut TestRng(1),
        );

        // Act
        let result = host
            .start(
                requester.start(),
                pin(1),
                [3; 32],
                info("host"),
                now,
                &mut TestRng(2),
            )
            .and_then(|answer| requester.answer(answer, pin(2), [3; 32]))
            .and_then(|confirm| host.confirm(confirm, now))
            .and_then(|paired| requester.done(paired.done()));

        // Assert
        assert_eq!(result.is_ok(), succeeds);
    }
}

#[test]
fn allow_sixth_attempt_blocks_until_oldest_expires() {
    // Arrange
    let now = Instant::now();
    let mut limiter = AttemptLimiter::new(2);
    for _ in 0..limits::PAIRING_ATTEMPTS_PER_MINUTE_PER_SOURCE {
        assert!(limiter.allow("source", now));
    }

    // Act
    let blocked = limiter.allow(
        "source",
        (now + limits::PAIRING_ATTEMPT_WINDOW)
            .checked_sub(Duration::from_nanos(1))
            .unwrap(),
    );
    let independent = limiter.allow("other", now);
    let renewed = limiter.allow("source", now + limits::PAIRING_ATTEMPT_WINDOW);

    // Assert
    assert!(!blocked);
    assert!(independent);
    assert!(renewed);
}

#[test]
fn allow_source_churn_bounds_memory_without_evicting_live_limits() {
    // Arrange
    let now = Instant::now();
    let mut limiter = AttemptLimiter::new(1);
    assert!(limiter.allow("first", now));

    // Act
    let blocked = limiter.allow("second", now);
    let renewed = limiter.allow("second", now + limits::PAIRING_ATTEMPT_WINDOW);

    // Assert
    assert!(!blocked);
    assert!(renewed);
    assert_eq!(limiter.sources.len(), 1);
}

mod validation;
