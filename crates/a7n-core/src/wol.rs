// SPDX-License-Identifier: GPL-3.0-or-later

//! Wake-on-LAN packet encoding and hardware address validation.

/// Ignore missing, malformed, all-zero and multicast hardware addresses.
#[must_use]
pub fn hw_addr(bytes: &[u8]) -> Option<[u8; 6]> {
    let mac: [u8; 6] = bytes.try_into().ok()?;
    (mac != [0; 6] && mac[0] & 1 == 0).then_some(mac)
}

/// Six synchronization bytes followed by sixteen copies of the address.
#[must_use]
pub fn magic_packet(mac: [u8; 6]) -> [u8; 102] {
    let mut packet = [0xff; 102];
    for chunk in packet[6..].chunks_exact_mut(6) {
        chunk.copy_from_slice(&mac);
    }
    packet
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_packet_valid_address_repeats_after_sync() {
        // Arrange
        let mac = [2, 3, 4, 5, 6, 7];

        // Act
        let packet = magic_packet(mac);

        // Assert
        assert_eq!(&packet[..6], &[0xff; 6]);
        assert_eq!(packet.len(), 102);
        assert!(packet[6..].chunks_exact(6).all(|chunk| chunk == mac));
    }
}
