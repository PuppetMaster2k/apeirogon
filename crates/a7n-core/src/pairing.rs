// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure pairing state machines (PROTOCOL.md 5). The caller supplies time,
//! certificate pins, TLS exporter binding, and a CSPRNG; no I/O happens here.

use std::{
    collections::{HashMap, VecDeque},
    fmt,
    hash::Hash,
    time::Instant,
};

use a7n_proto::{
    Pin, limits,
    v1::{PairAnswer, PairConfirm, PairDone, PairError, PairStart, PeerInfo},
};
use rand_core::{CryptoRng, RngCore};
use zeroize::{Zeroize, Zeroizing};

mod code;
mod crypto;

pub use code::{InvalidPairingCode, PairingCode};
#[cfg(test)]
mod tests;

use crypto::{Confirmation, TAG_LENGTH};

/// Peer whose proof has verified. Machine id must come from its TLS certificate.
#[derive(Debug)]
pub struct Peer {
    pub pin: Pin,
    pub info: PeerInfo,
}

/// Host success: persist `peer` before consuming this value with [`Self::done`].
pub struct Paired {
    pub peer: Peer,
    confirmation: Confirmation,
}

impl Paired {
    /// Produce the storage acknowledgement only after the caller persisted `peer`.
    #[must_use]
    pub fn done(self) -> PairDone {
        PairDone {
            error: PairError::None.into(),
            tag: self.confirmation.tag(true, true),
        }
    }
}

impl fmt::Debug for Paired {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Paired").finish_non_exhaustive()
    }
}

/// One host code window, permitting at most one exchange.
pub struct HostPairing {
    password: Zeroizing<curve25519_dalek::scalar::Scalar>,
    qr: bool,
    pin: Pin,
    opened_at: Instant,
    attempts: usize,
    pending: Option<(Confirmation, Peer)>,
    closed: bool,
}

impl fmt::Debug for HostPairing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostPairing").finish_non_exhaustive()
    }
}

impl HostPairing {
    #[must_use]
    pub fn new(code: PairingCode, qr: Option<[u8; 16]>, pin: Pin, opened_at: Instant) -> Self {
        let qr = Zeroizing::new(qr);
        let password = Zeroizing::new(crypto::password(&code, qr.as_ref()));
        drop(code);
        Self {
            password,
            qr: qr.is_some(),
            pin,
            opened_at,
            attempts: 0,
            pending: None,
            closed: false,
        }
    }

    /// Permanently leave pairing mode, including on transport or decoding failure.
    pub fn refuse(&mut self) {
        self.closed = true;
        self.pending = None;
        self.password.zeroize();
    }

    /// Consume the sole guess and produce the host proof.
    ///
    /// # Errors
    /// Returns `NotPairing` after the window is closed, otherwise only `Refused`.
    #[allow(
        clippy::too_many_arguments,
        reason = "the I/O shell supplies explicit identity, binding, time and randomness"
    )]
    pub fn start(
        &mut self,
        start: PairStart,
        requester_pin: Pin,
        ctx: [u8; 32],
        info: PeerInfo,
        now: Instant,
        rng: &mut (impl CryptoRng + RngCore),
    ) -> Result<PairAnswer, PairError> {
        if self.closed {
            self.refuse();
            return Err(PairError::NotPairing);
        }
        if self.attempts >= limits::PAIRING_CODE_GUESSES
            || self.pending.is_some()
            || self.expired(now)
        {
            self.refuse();
            return Err(PairError::Refused);
        }
        self.attempts += 1;
        // A failed start must close the window even when validation exits early.
        self.closed = true;
        let result = self.answer(start, requester_pin, ctx, info, rng);
        if result.is_err() {
            self.refuse();
        }
        result
    }

    fn answer(
        &mut self,
        start: PairStart,
        pin: Pin,
        ctx: [u8; 32],
        info: PeerInfo,
        rng: &mut (impl CryptoRng + RngCore),
    ) -> Result<PairAnswer, PairError> {
        let point = crypto::decode_share(&start.share).ok_or(PairError::Refused)?;
        if start.qr != self.qr {
            return Err(PairError::Refused);
        }
        let peer = Peer {
            pin,
            info: start.info.ok_or(PairError::Refused)?,
        };
        let secret = Zeroizing::new(crypto::random_scalar(rng));
        let share = crypto::share(&secret, &self.password, false);
        // RFC 9382 section 3.3: K = h*y*(pA - w*M).
        let shared = (*secret * (point - *self.password * crypto::mask(true))).mul_by_cofactor();
        let confirmation = Confirmation::new(
            (pin.as_bytes(), self.pin.as_bytes()),
            (&start.share, &share),
            shared,
            &self.password,
            &ctx,
        )
        .ok_or(PairError::Refused)?;
        let tag = confirmation.tag(true, false);
        self.password.zeroize();
        self.pending = Some((confirmation, peer));
        Ok(PairAnswer {
            error: PairError::None.into(),
            share: share.to_vec(),
            tag,
            info: Some(info),
        })
    }

    /// Verify the requester proof, returning a peer to persist before `done()`.
    ///
    /// # Errors
    /// Every failure returns only `Refused` and permanently ends pairing mode.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "consume the one-use wire message at this state transition"
    )]
    pub fn confirm(&mut self, confirm: PairConfirm, now: Instant) -> Result<Paired, PairError> {
        let pending = self.pending.take();
        self.refuse();
        let (confirmation, peer) = pending.ok_or(PairError::Refused)?;
        if self.expired(now)
            || confirm.error != i32::from(PairError::None)
            || !confirmation.verify(&confirm.tag, false, false)
        {
            return Err(PairError::Refused);
        }
        Ok(Paired { peer, confirmation })
    }

    fn expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.opened_at) >= limits::PAIRING_CODE_DURATION
    }
}

/// Requester exchange. No peer is returned until the storage MAC verifies.
pub struct RequesterPairing {
    secret: Zeroizing<curve25519_dalek::scalar::Scalar>,
    password: Zeroizing<curve25519_dalek::scalar::Scalar>,
    pin: Pin,
    start: PairStart,
    answered: bool,
    pending: Option<(Confirmation, Peer)>,
}

impl fmt::Debug for RequesterPairing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RequesterPairing").finish_non_exhaustive()
    }
}

impl RequesterPairing {
    pub fn new(
        code: PairingCode,
        qr: Option<[u8; 16]>,
        pin: Pin,
        info: PeerInfo,
        rng: &mut (impl CryptoRng + RngCore),
    ) -> Self {
        let qr = Zeroizing::new(qr);
        let password = Zeroizing::new(crypto::password(&code, qr.as_ref()));
        drop(code);
        let secret = Zeroizing::new(crypto::random_scalar(rng));
        let start = PairStart {
            share: crypto::share(&secret, &password, true).to_vec(),
            info: Some(info),
            qr: qr.is_some(),
        };
        Self {
            secret,
            password,
            pin,
            start,
            answered: false,
            pending: None,
        }
    }

    /// The start message for this exchange; send it only once.
    #[must_use]
    pub fn start(&self) -> PairStart {
        self.start.clone()
    }

    /// Verify the host proof and produce the requester proof.
    ///
    /// # Errors
    /// Every failure returns only `Refused`; another answer cannot be tried.
    pub fn answer(
        &mut self,
        answer: PairAnswer,
        host_pin: Pin,
        ctx: [u8; 32],
    ) -> Result<PairConfirm, PairError> {
        let pending = if self.answered {
            Err(PairError::Refused)
        } else {
            self.verify_answer(answer, host_pin, ctx)
        };
        self.answered = true;
        self.secret.zeroize();
        self.password.zeroize();
        self.pending = None;
        let (confirmation, peer) = pending?;
        let tag = confirmation.tag(false, false);
        self.pending = Some((confirmation, peer));
        Ok(PairConfirm {
            tag,
            error: PairError::None.into(),
        })
    }

    fn verify_answer(
        &self,
        answer: PairAnswer,
        pin: Pin,
        ctx: [u8; 32],
    ) -> Result<(Confirmation, Peer), PairError> {
        if answer.error != i32::from(PairError::None) || answer.tag.len() != TAG_LENGTH {
            return Err(PairError::Refused);
        }
        let point = crypto::decode_share(&answer.share).ok_or(PairError::Refused)?;
        let peer = Peer {
            pin,
            info: answer.info.ok_or(PairError::Refused)?,
        };
        // RFC 9382 section 3.3: K = h*x*(pB - w*N).
        let shared =
            (*self.secret * (point - *self.password * crypto::mask(false))).mul_by_cofactor();
        let confirmation = Confirmation::new(
            (self.pin.as_bytes(), pin.as_bytes()),
            (&self.start.share, &answer.share),
            shared,
            &self.password,
            &ctx,
        )
        .ok_or(PairError::Refused)?;
        if !confirmation.verify(&answer.tag, true, false) {
            return Err(PairError::Refused);
        }
        Ok((confirmation, peer))
    }

    /// Return the authenticated host only after verifying its storage proof.
    ///
    /// # Errors
    /// Returns only `Refused` on an out-of-order, rejected or invalid done message.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "consume the one-use wire message at this state transition"
    )]
    pub fn done(self, done: PairDone) -> Result<Peer, PairError> {
        let (confirmation, peer) = self.pending.ok_or(PairError::Refused)?;
        if done.error != i32::from(PairError::None) || !confirmation.verify(&done.tag, true, true) {
            return Err(PairError::Refused);
        }
        Ok(peer)
    }
}

/// Rolling one-minute source limiter. Capacity bounds memory even under source churn.
#[derive(Debug)]
pub struct AttemptLimiter<K> {
    sources: HashMap<K, VecDeque<Instant>>,
    max_sources: usize,
}

impl<K: Eq + Hash> AttemptLimiter<K> {
    #[must_use]
    pub fn new(max_sources: usize) -> Self {
        Self {
            sources: HashMap::new(),
            max_sources,
        }
    }

    /// Count an allowed attempt; reject new sources if every bounded slot is live.
    pub fn allow(&mut self, source: K, now: Instant) -> bool {
        self.sources.retain(|_, attempts| {
            while attempts.front().is_some_and(|&at| {
                now.saturating_duration_since(at) >= limits::PAIRING_ATTEMPT_WINDOW
            }) {
                attempts.pop_front();
            }
            !attempts.is_empty()
        });
        if !self.sources.contains_key(&source) && self.sources.len() >= self.max_sources {
            return false;
        }
        let attempts = self.sources.entry(source).or_default();
        if attempts.len() >= limits::PAIRING_ATTEMPTS_PER_MINUTE_PER_SOURCE {
            return false;
        }
        attempts.push_back(now);
        true
    }
}
