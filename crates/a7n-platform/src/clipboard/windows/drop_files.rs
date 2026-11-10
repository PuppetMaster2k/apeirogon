// SPDX-License-Identifier: GPL-3.0-or-later

//! The `CF_HDROP` payload: a `DROPFILES` header followed by a double-NUL
//! terminated list of UTF-16 paths. Pure.

use std::path::PathBuf;

/// `sizeof(DROPFILES)`: `pFiles`, `pt.x`, `pt.y`, `fNC`, `fWide`, four bytes each.
const HEADER_LEN: u32 = 20;
const POINT_ORIGIN: [u32; 2] = [0, 0];
const NOT_NON_CLIENT: u32 = 0;
const WIDE_CHARACTERS: u32 = 1;

/// `None` when no path is usable, since an empty `CF_HDROP` confuses Explorer.
pub(super) fn build(paths: &[PathBuf]) -> Option<Vec<u8>> {
    let mut units: Vec<u16> = Vec::new();
    for path in paths {
        let wide: Vec<u16> = path.to_string_lossy().encode_utf16().collect();
        // An embedded NUL would end the list early.
        if wide.is_empty() || wide.contains(&0) {
            continue;
        }
        units.extend(wide);
        units.push(0);
    }
    if units.is_empty() {
        return None;
    }
    units.push(0);
    let header = [
        HEADER_LEN,
        POINT_ORIGIN[0],
        POINT_ORIGIN[1],
        NOT_NON_CLIENT,
        WIDE_CHARACTERS,
    ];
    let mut bytes: Vec<u8> = header.into_iter().flat_map(u32::to_le_bytes).collect();
    bytes.extend(units.into_iter().flat_map(u16::to_le_bytes));
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_two_paths_writes_header_and_double_nul_list() {
        // Arrange
        let paths = [PathBuf::from("C:/a"), PathBuf::from("D:/\u{e9}")];

        // Act
        let bytes = build(&paths).expect("paths are usable");

        // Assert
        let mut expected: Vec<u8> = [20_u32, 0, 0, 0, 1]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        let list = "C:/a\0D:/\u{e9}\0\0";
        expected.extend(list.encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(bytes, expected);
    }
}
