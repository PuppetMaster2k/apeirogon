// SPDX-License-Identifier: MIT OR Apache-2.0

use std::fmt;
use std::str::FromStr;

use crate::ProtoError;

/// First 16 bytes of the SHA-256 of a device's public key (PROTOCOL.md 3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MachineId([u8; MachineId::LEN]);

impl MachineId {
    /// Length in bytes: the SHA-256 prefix kept from the key (PROTOCOL.md 3.1).
    pub const LEN: usize = 16;

    /// The raw 16 bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; Self::LEN] {
        &self.0
    }
}

impl TryFrom<&[u8]> for MachineId {
    type Error = ProtoError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let array: [u8; Self::LEN] = bytes
            .try_into()
            .map_err(|_| ProtoError::BadMachineId(bytes.len()))?;
        Ok(Self(array))
    }
}

impl fmt::Display for MachineId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for MachineId {
    type Err = ProtoError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.len() != Self::LEN * 2 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(ProtoError::BadMachineIdHex);
        }
        let mut bytes = [0; Self::LEN];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
                .map_err(|_| ProtoError::BadMachineIdHex)?;
        }
        Ok(Self(bytes))
    }
}

/// SHA-256 of a device's full certificate; pairing stores it (PROTOCOL.md 3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pin([u8; 32]);

impl Pin {
    /// The raw 32 bytes.
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

/// Number that rises with every move; older routes are ignored (section 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Route(pub u64);

/// Clipboard Lamport counter (section 10). Compare `(Generation, origin
/// MachineId)` tuples: higher generation wins, a tie goes to the larger origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Generation(pub u64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_id_from_str_hex_input_validates_and_formats_lowercase() {
        // Arrange
        let cases = [
            ("0123456789abcdef0123456789abcdef", true),
            ("0123456789ABCDEF0123456789ABCDEF", true),
            ("00000000000000000000000000000000", true),
            ("", false),
            ("0123456789abcdef", false),
            ("0123456789abcdef0123456789abcdef00", false),
            ("g123456789abcdef0123456789abcdef", false),
            ("éééééééééééééééé", false),
        ];
        for (input, valid) in cases {
            // Act
            let result = input.parse::<MachineId>();

            // Assert
            if valid {
                assert_eq!(result.unwrap().to_string(), input.to_ascii_lowercase());
            } else {
                assert!(
                    matches!(result, Err(ProtoError::BadMachineIdHex)),
                    "{input}"
                );
            }
        }
    }

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

    #[test]
    fn generation_max_equal_generation_larger_origin_wins() {
        // Arrange
        let low = MachineId::try_from(&[1u8; 16][..]).unwrap();
        let high = MachineId::try_from(&[2u8; 16][..]).unwrap();

        // Act
        let winner = (Generation(5), low).max((Generation(5), high));

        // Assert
        assert_eq!(winner.1, high);
    }
}
