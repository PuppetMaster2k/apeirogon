// SPDX-License-Identifier: GPL-3.0-or-later

//! Default physical modifier mapping between platforms.

use a7n_proto::v1::Platform;

pub const LEFT_CTRL: u32 = 0x0007_00e0;
pub const LEFT_META: u32 = 0x0007_00e3;
pub const RIGHT_CTRL: u32 = 0x0007_00e4;
pub const RIGHT_META: u32 = 0x0007_00e7;

const CMD_CTRL_SWAP: &[(u32, u32)] = &[
    (LEFT_META, LEFT_CTRL),
    (LEFT_CTRL, LEFT_META),
    (RIGHT_META, RIGHT_CTRL),
    (RIGHT_CTRL, RIGHT_META),
];

#[must_use]
pub fn default_modifier_remap(sender: Platform, receiver: Platform) -> &'static [(u32, u32)] {
    match (sender, receiver) {
        (Platform::Macos, Platform::Windows | Platform::Linux)
        | (Platform::Windows | Platform::Linux, Platform::Macos) => CMD_CTRL_SWAP,
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_modifier_remap_platform_pairs_swap_only_between_mac_and_pc() {
        // Arrange
        let platforms = [
            Platform::Macos,
            Platform::Windows,
            Platform::Linux,
            Platform::Unspecified,
        ];
        let expected_swap = [
            (0x0007_00e3, 0x0007_00e0),
            (0x0007_00e0, 0x0007_00e3),
            (0x0007_00e7, 0x0007_00e4),
            (0x0007_00e4, 0x0007_00e7),
        ];
        let swaps = [
            [false, true, true, false],
            [true, false, false, false],
            [true, false, false, false],
            [false, false, false, false],
        ];
        for (i, sender) in platforms.into_iter().enumerate() {
            for (j, receiver) in platforms.into_iter().enumerate() {
                // Act
                let remap = default_modifier_remap(sender, receiver);

                // Assert
                let expected = if swaps[i][j] {
                    expected_swap.as_slice()
                } else {
                    &[]
                };
                assert_eq!(remap, expected, "{sender:?} → {receiver:?}");
            }
        }
    }
}
