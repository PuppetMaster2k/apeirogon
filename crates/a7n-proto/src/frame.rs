// SPDX-License-Identifier: MIT OR Apache-2.0

//! Varint length-prefixed framing for streams, prefix-less datagrams
//! (PROTOCOL.md section 2).

use prost::Message;

use crate::ProtoError;
use crate::limits;
use crate::v1::{Envelope, Motion, envelope::Body};

/// Longest valid protobuf varint.
const MAX_VARINT_BYTES: usize = 10;
/// Continuation flag of a varint byte.
const VARINT_MORE: u8 = 0x80;
/// Payload bits of a varint byte.
const VARINT_PAYLOAD: u8 = 0x7f;
/// Bits of payload per varint byte.
const VARINT_SHIFT: u32 = 7;

/// Which stream a frame travels on; selects its size limit so callers cannot
/// pick the wrong number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    /// The first `Hello` on a new control stream.
    FirstHello,
    /// Any later control frame.
    Control,
    /// A pairing stream frame.
    Pairing,
    /// A transfer stream frame.
    Transfer,
}

impl FrameKind {
    /// Largest body in bytes for this kind of frame.
    #[must_use]
    pub fn max_frame(self) -> usize {
        match self {
            Self::FirstHello => limits::FIRST_HELLO_MAX,
            Self::Control | Self::Pairing => limits::CONTROL_FRAME_MAX,
            Self::Transfer => limits::TRANSFER_FRAME_MAX,
        }
    }
}

/// Appends `msg` to `buf` as a varint length followed by the body.
pub fn encode_frame<M: Message>(msg: &M, buf: &mut Vec<u8>) {
    let len = msg.encoded_len();
    buf.reserve(len + MAX_VARINT_BYTES);
    prost::encoding::encode_varint(len as u64, buf);
    msg.encode_raw(buf);
}

/// Reads one frame from the front of `buf`.
///
/// Returns `Ok(None)` while more bytes are needed, else the message and the
/// number of bytes consumed. The length prefix is checked against the limit
/// for `kind` before the body is waited for, so nothing oversized is buffered.
///
/// # Errors
///
/// [`ProtoError::BadLengthPrefix`], [`ProtoError::FrameTooLarge`] or
/// [`ProtoError::Decode`]; the caller closes the link.
pub fn decode_frame<M: Message + Default>(
    buf: &[u8],
    kind: FrameKind,
) -> Result<Option<(M, usize)>, ProtoError> {
    let Some((len, prefix_len)) = read_varint(buf)? else {
        return Ok(None);
    };
    let max = kind.max_frame();
    let len = usize::try_from(len)
        .ok()
        .filter(|&len| len <= max)
        .ok_or(ProtoError::FrameTooLarge { len, max })?;
    let end = prefix_len + len;
    let Some(body) = buf.get(prefix_len..end) else {
        return Ok(None);
    };
    Ok(Some((M::decode(body)?, end)))
}

/// Appends a datagram: one `Envelope`, no length prefix.
pub fn encode_datagram(envelope: &Envelope, buf: &mut Vec<u8>) {
    buf.reserve(envelope.encoded_len());
    envelope.encode_raw(buf);
}

/// Decodes a datagram. Anything but `Motion` is ignored (`Ok(None)`).
///
/// # Errors
///
/// [`ProtoError::Decode`] when the bytes are not an `Envelope`.
pub fn decode_datagram(datagram: &[u8]) -> Result<Option<Motion>, ProtoError> {
    Ok(match Envelope::decode(datagram)?.body {
        Some(Body::Motion(motion)) => Some(motion),
        _ => None,
    })
}

/// Reads a varint; `Ok(None)` if `buf` ends before it does.
fn read_varint(buf: &[u8]) -> Result<Option<(u64, usize)>, ProtoError> {
    let mut value = 0u64;
    for (index, &byte) in buf.iter().take(MAX_VARINT_BYTES).enumerate() {
        let shift = VARINT_SHIFT * u32::try_from(index).expect("index is below 10");
        let payload = u64::from(byte & VARINT_PAYLOAD);
        if shift == VARINT_SHIFT * 9 && payload > 1 {
            return Err(ProtoError::BadLengthPrefix);
        }
        value |= payload << shift;
        if byte & VARINT_MORE == 0 {
            return Ok(Some((value, index + 1)));
        }
    }
    if buf.len() >= MAX_VARINT_BYTES {
        return Err(ProtoError::BadLengthPrefix);
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v1::Ack;

    fn ack() -> Ack {
        Ack {
            seq: 7,
            held_us: 3,
            locked: false,
        }
    }

    #[test]
    fn decode_frame_encoded_frame_round_trips() {
        // Arrange
        let mut buf = Vec::new();
        encode_frame(&ack(), &mut buf);

        // Act
        let result = decode_frame::<Ack>(&buf, FrameKind::Control).unwrap();

        // Assert
        assert_eq!(result, Some((ack(), buf.len())));
    }

    #[test]
    fn decode_frame_partial_body_returns_none() {
        // Arrange
        let mut buf = Vec::new();
        encode_frame(&ack(), &mut buf);
        buf.pop();

        // Act
        let result = decode_frame::<Ack>(&buf, FrameKind::Control).unwrap();

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn decode_frame_oversized_prefix_fails_before_body_arrives() {
        // Arrange: only the prefix (4097 as varint) is present, no body bytes.
        let prefix = [0x81, 0x20];

        // Act
        let result = decode_frame::<Envelope>(&prefix, FrameKind::FirstHello);

        // Assert
        assert!(matches!(
            result,
            Err(ProtoError::FrameTooLarge {
                len: 4097,
                max: 4096
            })
        ));
    }

    #[test]
    fn decode_frame_endless_varint_is_rejected() {
        // Arrange
        let prefix = [0xff; MAX_VARINT_BYTES];

        // Act
        let result = decode_frame::<Envelope>(&prefix, FrameKind::Control);

        // Assert
        assert!(matches!(result, Err(ProtoError::BadLengthPrefix)));
    }

    #[test]
    fn decode_datagram_non_motion_is_ignored() {
        // Arrange
        let envelope = Envelope {
            body: Some(Body::Ack(ack())),
        };
        let mut buf = Vec::new();
        encode_datagram(&envelope, &mut buf);

        // Act
        let result = decode_datagram(&buf).unwrap();

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn decode_datagram_unknown_oneof_case_is_ignored() {
        // Arrange: field 99 (tag 0x9a 0x06), length-delimited, 1 byte; plus
        // unknown scalar field 98 (tag 0x90 0x06) varint 1. Neither is known.
        let bytes = [0x9a, 0x06, 0x01, 0x00, 0x90, 0x06, 0x01];

        // Act
        let result = decode_datagram(&bytes);

        // Assert
        assert!(matches!(result, Ok(None)));
    }
}
