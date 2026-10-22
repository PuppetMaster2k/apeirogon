// SPDX-License-Identifier: GPL-3.0-or-later

use curve25519_dalek::{
    constants::ED25519_BASEPOINT_POINT,
    edwards::{CompressedEdwardsY, EdwardsPoint},
    scalar::Scalar,
    traits::IsIdentity,
};
use hkdf::Hkdf;
use hmac::{Hmac, KeyInit, Mac};
use rand_core::{CryptoRng, RngCore};
use sha2::{Digest, Sha256, Sha512};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use super::PairingCode;

pub(super) const SHARE_LENGTH: usize = 32;
pub(super) const TAG_LENGTH: usize = 32;
const KEY_LENGTH: usize = 16;
// RFC 9382 section 6, compressed edwards25519 points.
const M: [u8; SHARE_LENGTH] = [
    0xd0, 0x48, 0x03, 0x2c, 0x6e, 0xa0, 0xb6, 0xd6, 0x97, 0xdd, 0xc2, 0xe8, 0x6b, 0xda, 0x85, 0xa3,
    0x3a, 0xda, 0xc9, 0x20, 0xf1, 0xbf, 0x18, 0xe1, 0xb0, 0xc6, 0xd1, 0x66, 0xa5, 0xce, 0xcd, 0xaf,
];
const N: [u8; SHARE_LENGTH] = [
    0xd3, 0xbf, 0xb5, 0x18, 0xf4, 0x4f, 0x34, 0x30, 0xf2, 0x9d, 0x0c, 0x92, 0xaf, 0x50, 0x38, 0x65,
    0xa1, 0xed, 0x32, 0x81, 0xdc, 0x69, 0xb3, 0x5d, 0xd8, 0x68, 0xba, 0x85, 0xf8, 0x86, 0xc4, 0xab,
];

pub(super) fn mask(requester: bool) -> EdwardsPoint {
    CompressedEdwardsY(if requester { M } else { N })
        .decompress()
        .expect("RFC 9382 section 6 constants are valid compressed points")
}

pub(super) fn password(code: &PairingCode, qr: Option<&[u8; 16]>) -> Scalar {
    // RFC 9382 section 3.2 leaves this to the application: SHA-512(code || QR),
    // interpreted little-endian and reduced mod l (512 bits avoids reduction bias).
    let mut hash = Sha512::new();
    hash.update(code.0);
    if let Some(qr) = qr {
        hash.update(qr);
    }
    let digest = Zeroizing::new(<[u8; 64]>::from(hash.finalize()));
    Scalar::from_bytes_mod_order_wide(&digest)
}

pub(super) fn random_scalar(rng: &mut (impl CryptoRng + RngCore)) -> Scalar {
    loop {
        let mut bytes = Zeroizing::new([0; 32]);
        rng.fill_bytes(bytes.as_mut());
        // Rejection sampling, RFC 9382 section 7. Mask only bits above l's bit length.
        bytes[31] &= 0x1f;
        if let Some(scalar) = Option::<Scalar>::from(Scalar::from_canonical_bytes(*bytes))
            && scalar != Scalar::ZERO
        {
            return scalar;
        }
    }
}

pub(super) fn share(secret: &Scalar, password: &Scalar, requester: bool) -> [u8; SHARE_LENGTH] {
    (secret * ED25519_BASEPOINT_POINT + password * mask(requester))
        .compress()
        .to_bytes()
}

pub(super) fn decode_share(bytes: &[u8]) -> Option<EdwardsPoint> {
    let encoded: [u8; SHARE_LENGTH] = bytes.try_into().ok()?;
    let point = CompressedEdwardsY(encoded).decompress()?;
    // Reject noncanonical encodings and points trivial in the quotient group.
    (point.compress().to_bytes() == encoded && !point.is_small_order()).then_some(point)
}

#[derive(Zeroize, ZeroizeOnDrop)]
pub(super) struct Confirmation {
    transcript: Vec<u8>,
    requester: [u8; KEY_LENGTH],
    host: [u8; KEY_LENGTH],
}

impl Confirmation {
    pub(super) fn new(
        identities: (&[u8], &[u8]),
        shares: (&[u8], &[u8]),
        key_point: EdwardsPoint,
        password: &Scalar,
        ctx: &[u8; 32],
    ) -> Option<Self> {
        if key_point.is_identity() {
            return None;
        }
        let mut w = Zeroizing::new(password.to_bytes());
        w.reverse();
        let k = Zeroizing::new(key_point.compress().to_bytes());
        let mut transcript = Zeroizing::new(Vec::new());
        // RFC 9382 sections 3.2-3.3: u64 LE lengths, points compressed, w BE.
        for field in [identities.0, identities.1, shares.0, shares.1, &*k, &*w] {
            transcript.extend_from_slice(&u64::try_from(field.len()).ok()?.to_le_bytes());
            transcript.extend_from_slice(field);
        }
        let digest = Zeroizing::new(<[u8; 32]>::from(Sha256::digest(&*transcript)));
        let mut keys = Zeroizing::new([0; 2 * KEY_LENGTH]);
        // RFC 9382 section 4: Ke || Ka = SHA256(TT); only Ka feeds HKDF.
        Hkdf::<Sha256>::new(None, &digest[KEY_LENGTH..])
            .expand_multi_info(&[b"ConfirmationKeys", ctx], keys.as_mut())
            .ok()?;
        Some(Self {
            transcript: std::mem::take(&mut *transcript),
            requester: keys[..KEY_LENGTH].try_into().ok()?,
            host: keys[KEY_LENGTH..].try_into().ok()?,
        })
    }

    fn mac(&self, host: bool, stored: bool) -> Hmac<Sha256> {
        let key = if host { &self.host } else { &self.requester };
        let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts every key length");
        mac.update(if stored { b"stored" } else { &self.transcript });
        mac
    }

    pub(super) fn tag(&self, host: bool, stored: bool) -> Vec<u8> {
        self.mac(host, stored).finalize().into_bytes().to_vec()
    }

    pub(super) fn verify(&self, tag: &[u8], host: bool, stored: bool) -> bool {
        tag.len() == TAG_LENGTH && self.mac(host, stored).verify_slice(tag).is_ok()
    }
}
