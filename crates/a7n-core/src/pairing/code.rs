// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;

use a7n_proto::limits;
use rand_core::{CryptoRng, RngCore};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Human-entered decimal secret. Debug output deliberately hides the code.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct PairingCode(pub(super) [u8; limits::PAIRING_CODE_LENGTH]);

/// Invalid pairing-code input; never includes the supplied secret.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("pairing code must contain exactly six ASCII digits")]
pub struct InvalidPairingCode;

impl PairingCode {
    /// Generate independent uniform decimal digits, without modulo bias.
    pub fn generate(rng: &mut (impl CryptoRng + RngCore)) -> Self {
        const DECIMAL_RADIX: u8 = 10;
        const UNBIASED_END: u8 = u8::MAX - u8::MAX % DECIMAL_RADIX;
        let mut digits = [0; limits::PAIRING_CODE_LENGTH];
        for digit in &mut digits {
            loop {
                let mut byte = [0];
                rng.fill_bytes(&mut byte);
                if byte[0] < UNBIASED_END {
                    *digit = b'0' + byte[0] % DECIMAL_RADIX;
                    break;
                }
            }
        }
        Self(digits)
    }

    /// Parse exactly the displayed ASCII digits, retaining leading zeroes.
    ///
    /// # Errors
    /// Returns an opaque error for any other input.
    pub fn parse(input: &str) -> Result<Self, InvalidPairingCode> {
        let digits = input
            .as_bytes()
            .try_into()
            .map_err(|_| InvalidPairingCode)?;
        if !input.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(InvalidPairingCode);
        }
        Ok(Self(digits))
    }
}

impl fmt::Display for PairingCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(std::str::from_utf8(&self.0).expect("PairingCode contains only ASCII digits"))
    }
}

impl fmt::Debug for PairingCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PairingCode([redacted])")
    }
}
