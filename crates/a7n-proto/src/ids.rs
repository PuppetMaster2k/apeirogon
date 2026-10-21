// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::ProtoError;

/// First 16 bytes of the SHA-256 of a device's public key (PROTOCOL.md 3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MachineId([u8; 16]);

impl MachineId {
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl TryFrom<&[u8]> for MachineId {
    type Error = ProtoError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let array: [u8; 16] = bytes
            .try_into()
            .map_err(|_| ProtoError::BadMachineId(bytes.len()))?;
        Ok(Self(array))
    }
}

/// SHA-256 of a device's full certificate; pairing stores it (PROTOCOL.md 3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pin([u8; 32]);

impl Pin {
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl TryFrom<&[u8]> for Pin {
    type Error = ProtoError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let array: [u8; 32] = bytes
            .try_into()
            .map_err(|_| ProtoError::BadPin(bytes.len()))?;
        Ok(Self(array))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_id_try_from_wrong_length_returns_bad_machine_id() {
        // Arrange
        let bytes = [0u8; 15];

        // Act
        let result = MachineId::try_from(&bytes[..]);

        // Assert
        assert!(matches!(result, Err(ProtoError::BadMachineId(15))));
    }

    #[test]
    fn pin_try_from_wrong_length_returns_bad_pin() {
        // Arrange
        let bytes = [0u8; 31];

        // Act
        let result = Pin::try_from(&bytes[..]);

        // Assert
        assert!(matches!(result, Err(ProtoError::BadPin(31))));
    }
}