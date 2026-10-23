// SPDX-License-Identifier: GPL-3.0-or-later

//! `CF_HTML` offsets count bytes, including the ASCII header.

const PREFIX: &str = "<html><body><!--StartFragment-->";
const SUFFIX: &str = "<!--EndFragment--></body></html>";

pub(super) fn encode(fragment: &str) -> Vec<u8> {
    let start = header(0, 0, 0, 0).len();
    let fragment_start = start + PREFIX.len();
    let fragment_end = fragment_start + fragment.len();
    let end = fragment_end + SUFFIX.len();
    let mut data = header(start, end, fragment_start, fragment_end).into_bytes();
    data.extend_from_slice(PREFIX.as_bytes());
    data.extend_from_slice(fragment.as_bytes());
    data.extend_from_slice(SUFFIX.as_bytes());
    data.push(0);
    data
}

fn header(start: usize, end: usize, fragment_start: usize, fragment_end: usize) -> String {
    format!(
        "Version:1.0\r\nStartHTML:{start:010}\r\nEndHTML:{end:010}\r\nStartFragment:{fragment_start:010}\r\nEndFragment:{fragment_end:010}\r\n"
    )
}

pub(super) fn decode(data: &[u8]) -> Option<&[u8]> {
    let mut html_start = None;
    let mut html_end = None;
    let mut fragment_start = None;
    let mut fragment_end = None;
    let mut header_end = 0;
    for line in data.split_inclusive(|byte| *byte == b'\n') {
        header_end += line.len();
        let line = std::str::from_utf8(line).ok()?.trim();
        let (key, value) = line.split_once(':')?;
        let field = match key {
            "StartHTML" => &mut html_start,
            "EndHTML" => &mut html_end,
            "StartFragment" => &mut fragment_start,
            "EndFragment" => &mut fragment_end,
            _ => continue,
        };
        if field.is_some() {
            return None;
        }
        *field = Some(value.trim().parse::<isize>().ok()?);
        if html_start.is_some()
            && html_end.is_some()
            && fragment_start.is_some()
            && fragment_end.is_some()
        {
            break;
        }
    }
    let start = usize::try_from(fragment_start?).ok()?;
    let end = usize::try_from(fragment_end?).ok()?;
    let html_start = match html_start? {
        -1 => header_end,
        value => usize::try_from(value).ok()?,
    };
    let html_end = match html_end? {
        -1 => data.len(),
        value => usize::try_from(value).ok()?,
    };
    if html_start < header_end || start < html_start || end > html_end || html_end > data.len() {
        return None;
    }
    let fragment = data.get(start..end)?;
    std::str::from_utf8(fragment).ok()?;
    Some(fragment)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_encoded_unicode_round_trips_fragment() {
        // Arrange
        let fragment = "<b>Grüße 🌍</b>";
        let data = encode(fragment);

        // Act
        let decoded = decode(&data);

        // Assert
        assert_eq!(decoded, Some(fragment.as_bytes()));
    }

    #[test]
    fn encode_fragment_offsets_point_to_byte_boundaries() {
        // Arrange
        let fragment = "é";

        // Act
        let data = encode(fragment);

        // Assert
        let expected_header = "Version:1.0\r\nStartHTML:0000000105\r\nEndHTML:0000000171\r\nStartFragment:0000000137\r\nEndFragment:0000000139\r\n";
        assert!(data.starts_with(expected_header.as_bytes()));
        assert_eq!(&data[137..139], fragment.as_bytes());
        assert_eq!(data[171], 0);
    }

    #[test]
    fn decode_malformed_headers_returns_none() {
        // Arrange
        let cases: &[&[u8]] = &[
            b"",
            b"<b>no header</b>",
            b"StartHTML:0\nEndHTML:99\nStartFragment:3\nEndFragment:2\n",
            b"StartHTML:-2\nEndHTML:0\nStartFragment:0\nEndFragment:0\n",
            b"StartHTML:999999999999999999999999\n",
            b"StartHTML:0\nEndHTML:1\nStartFragment:no\nEndFragment:1\n",
            b"StartHTML:0\nStartHTML:0\nEndHTML:1\nStartFragment:0\nEndFragment:1\n",
        ];
        let valid = String::from_utf8(encode("é")).expect("encoded UTF-8 is valid");
        let invalid_offsets = [
            valid.replace("EndFragment:0000000139", "EndFragment:0000009999"),
            valid.replace("EndFragment:0000000139", "EndFragment:0000000136"),
            valid.replace("EndFragment:0000000139", "EndFragment:0000000138"),
            valid.replace("EndHTML:0000000171", "EndHTML:0000009999"),
        ];
        for data in cases
            .iter()
            .copied()
            .chain(invalid_offsets.iter().map(String::as_bytes))
        {
            // Act
            let decoded = decode(data);

            // Assert
            assert_eq!(decoded, None);
        }
    }
}
