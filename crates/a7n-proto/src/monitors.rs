// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{
    limits,
    v1::{MonitorRect, PeerInfo},
};

impl PeerInfo {
    /// Ignore the entire optional monitor field when any rectangle is invalid.
    /// Call once at the handshake decode boundary; invalid metadata is not a link error.
    pub fn discard_invalid_monitors(&mut self) {
        if self.monitors.len() > limits::MONITORS_MAX || !self.monitors.iter().all(valid_rect) {
            self.monitors.clear();
        }
    }
}

fn valid_rect(rect: &MonitorRect) -> bool {
    rect.x >= 0
        && rect.y >= 0
        && rect.width > 0
        && rect.height > 0
        && rect.x.checked_add_unsigned(rect.width).is_some()
        && rect.y.checked_add_unsigned(rect.height).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;

    #[test]
    fn discard_invalid_monitors_invalid_metadata_drops_only_monitors() {
        // Arrange
        let monitor = MonitorRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let cases = [
            (vec![monitor; limits::MONITORS_MAX + 1], 0),
            (
                vec![MonitorRect {
                    width: 0,
                    ..monitor
                }],
                0,
            ),
            (
                vec![MonitorRect {
                    height: 0,
                    ..monitor
                }],
                0,
            ),
            (
                vec![MonitorRect {
                    x: i32::MAX,
                    ..monitor
                }],
                0,
            ),
            (
                vec![MonitorRect {
                    y: i32::MAX,
                    ..monitor
                }],
                0,
            ),
            (vec![MonitorRect { x: -1, ..monitor }], 0),
            (vec![monitor; limits::MONITORS_MAX], limits::MONITORS_MAX),
            (vec![], 0),
        ];
        for (monitors, count) in cases {
            let bytes = PeerInfo {
                name: "desktop".into(),
                monitors,
                ..PeerInfo::default()
            }
            .encode_to_vec();
            let mut info = PeerInfo::decode(bytes.as_slice()).unwrap();

            // Act
            info.discard_invalid_monitors();

            // Assert
            assert_eq!(info.monitors.len(), count);
            assert_eq!(info.name, "desktop");
        }
    }
}
