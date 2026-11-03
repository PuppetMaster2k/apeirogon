// SPDX-License-Identifier: GPL-3.0-or-later

//! `text/uri-list` (RFC 2483) and the GNOME `x-special/gnome-copied-files`
//! variant: how file managers hand copied files to each other. Pure.

use std::path::{Path, PathBuf};

const SCHEME: &str = "file://";
const LOCALHOST: &str = "localhost";
const LINE_END: &[u8] = b"\r\n";
const COMMENT: u8 = b'#';
const PERCENT: u8 = b'%';
const HEX_RADIX: u32 = 16;
/// First line of the GNOME format; every copy is treated as a copy, never a cut.
const GNOME_COPY_VERB: &[u8] = b"copy";

/// Percent-encoded `file://` URIs, one per line, CRLF-terminated.
#[must_use]
pub fn encode(paths: &[PathBuf]) -> Vec<u8> {
    let mut out = Vec::new();
    for path in paths {
        out.extend_from_slice(&uri_of(path));
        out.extend_from_slice(LINE_END);
    }
    out
}

/// `copy` verb, then the URIs separated by `\n` without a trailing newline.
#[must_use]
pub fn encode_gnome_copied(paths: &[PathBuf]) -> Vec<u8> {
    let mut out = GNOME_COPY_VERB.to_vec();
    for path in paths {
        out.push(b'\n');
        out.extend_from_slice(&uri_of(path));
    }
    out
}

/// Local paths named by a URI list or GNOME copied-files payload. Comments,
/// other schemes, remote hosts and invalid percent-encoding are skipped.
#[must_use]
pub fn decode(bytes: &[u8]) -> Vec<PathBuf> {
    bytes
        .split(|byte| *byte == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
        .filter(|line| line.first() != Some(&COMMENT))
        .filter_map(path_of)
        .collect()
}

fn uri_of(path: &Path) -> Vec<u8> {
    let mut uri = SCHEME.as_bytes().to_vec();
    for byte in path_bytes(path) {
        if is_unreserved(byte) || byte == b'/' {
            uri.push(byte);
        } else {
            uri.extend_from_slice(format!("%{byte:02X}").as_bytes());
        }
    }
    uri
}

fn path_of(line: &[u8]) -> Option<PathBuf> {
    let rest = line.strip_prefix(SCHEME.as_bytes())?;
    let slash = rest.iter().position(|byte| *byte == b'/')?;
    let (host, encoded) = rest.split_at(slash);
    if !host.is_empty() && !host.eq_ignore_ascii_case(LOCALHOST.as_bytes()) {
        return None;
    }
    Some(path_from_bytes(&percent_decode(encoded)?))
}

fn percent_decode(encoded: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(encoded.len());
    let mut bytes = encoded.iter().copied();
    while let Some(byte) = bytes.next() {
        if byte != PERCENT {
            out.push(byte);
            continue;
        }
        let high = char::from(bytes.next()?).to_digit(HEX_RADIX)?;
        let low = char::from(bytes.next()?).to_digit(HEX_RADIX)?;
        out.push(u8::try_from(high * HEX_RADIX + low).ok()?);
    }
    Some(out)
}

fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

/// `C:\a b` becomes `/C:/a b`, the form `file://` URIs use for drive paths.
#[cfg(not(unix))]
fn path_bytes(path: &Path) -> Vec<u8> {
    let text = path.to_string_lossy().replace('\\', "/");
    if text.starts_with('/') {
        text.into_bytes()
    } else {
        format!("/{text}").into_bytes()
    }
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    let text = String::from_utf8_lossy(bytes).into_owned();
    let drive = text.as_bytes().get(2) == Some(&b':');
    PathBuf::from(if drive { &text[1..] } else { &text })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn encode_special_characters_percent_encodes_each_path() {
        // Arrange
        let cases = [
            ("/tmp/plain.txt", "file:///tmp/plain.txt"),
            ("/tmp/a b", "file:///tmp/a%20b"),
            ("/tmp/100%", "file:///tmp/100%25"),
            ("/tmp/#1", "file:///tmp/%231"),
            ("/tmp/\u{e9}", "file:///tmp/%C3%A9"),
        ];
        for (path, uri) in cases {
            // Act
            let encoded = encode(&[PathBuf::from(path)]);

            // Assert
            assert_eq!(encoded, format!("{uri}\r\n").into_bytes(), "{path}");
        }
    }

    #[test]
    fn decode_mixed_lines_keeps_only_local_file_paths() {
        // Arrange
        let list = "# comment\r\n\
            file:///tmp/a%20b\r\n\
            file://localhost/tmp/c\n\
            file://remote/tmp/d\r\n\
            https://example.com/e\r\n\
            file:///tmp/bad%zz\r\n\
            file:///tmp/%C3%A9\r\n\
            copy\n";

        // Act
        let paths = decode(list.as_bytes());

        // Assert
        let expected = ["/tmp/a b", "/tmp/c", "/tmp/\u{e9}"].map(PathBuf::from);
        assert_eq!(paths, expected);
    }

    #[test]
    fn encode_gnome_copied_roundtrips_through_decode() {
        // Arrange
        let paths = ["/tmp/a b", "/tmp/c"].map(PathBuf::from);

        // Act
        let encoded = encode_gnome_copied(&paths);

        // Assert
        assert_eq!(encoded, b"copy\nfile:///tmp/a%20b\nfile:///tmp/c");
        assert_eq!(decode(&encoded), paths);
    }
}
