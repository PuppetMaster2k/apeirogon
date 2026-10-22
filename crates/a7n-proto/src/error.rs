// SPDX-License-Identifier: MIT OR Apache-2.0

/// Everything that can go wrong when turning bytes from a peer into
/// checked protocol values. The caller closes the link on any of these.
#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    /// A machine id was not 16 bytes.
    #[error("machine id must be 16 bytes, got {0}")]
    BadMachineId(usize),
    /// A pin was not 32 bytes.
    #[error("pin must be 32 bytes, got {0}")]
    BadPin(usize),
    /// A frame's length prefix exceeds the limit for its stream.
    #[error("frame of {len} bytes exceeds the limit of {max}")]
    FrameTooLarge {
        /// Length announced by the prefix.
        len: u64,
        /// Limit for this kind of frame.
        max: usize,
    },
    /// The length prefix is not a valid varint.
    #[error("malformed varint length prefix")]
    BadLengthPrefix,
    /// The body is not a valid protobuf message.
    #[error(transparent)]
    Decode(#[from] prost::DecodeError),
}
