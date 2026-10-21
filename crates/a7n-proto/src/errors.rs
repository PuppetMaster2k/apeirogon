// SPDX-License-Identifier: MIT OR Apache-2.0

/// Everything that can go wrong when turning bytes from a peer into
/// checked protocol values.
#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    #[error("machine id must be 16 bytes, got {0}")]
    BadMachineId(usize),
    #[error("pin must be 32 bytes, got {0}")]
    BadPin(usize),
}