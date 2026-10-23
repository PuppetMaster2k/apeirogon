// SPDX-License-Identifier: GPL-3.0-or-later

//! Text clipboard formats shared by the X11 and Wayland backends.

use a7n_core::clipboard::{TEXT_HTML, TEXT_PLAIN};

/// Password managers offer this format next to a secret; such a copy is never
/// announced, and the exclusion cannot be turned off.
pub(super) const SENSITIVE_MARKER: &str = "x-kde-passwordManagerHint";
pub(super) const HTML_ALIAS: &str = "text/html";
pub(super) const HTML_UTF8_ALIAS: &str = "text/html;charset=utf-8";

/// Names for plain text, best first. The legacy `UTF8_STRING` still shows up
/// from `XWayland` clients.
const PLAIN_NAMES: [&str; 3] = [TEXT_PLAIN, "text/plain", "UTF8_STRING"];
const HTML_NAMES: [&str; 2] = [HTML_UTF8_ALIAS, HTML_ALIAS];

/// Every name an item is offered under, best first.
pub(super) fn names_of(item_mime: &str) -> &'static [&'static str] {
    match item_mime {
        TEXT_PLAIN => &PLAIN_NAMES,
        TEXT_HTML => &HTML_NAMES,
        _ => &[],
    }
}

/// The item format a requested name stands for.
pub(super) fn item_mime_of(name: &str) -> Option<&'static str> {
    [TEXT_PLAIN, TEXT_HTML]
        .into_iter()
        .find(|item| names_of(item).contains(&name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_mime_of_aliases_maps_only_supported_names() {
        // Arrange
        let cases = [
            ("UTF8_STRING", Some(TEXT_PLAIN)),
            ("text/plain", Some(TEXT_PLAIN)),
            (TEXT_PLAIN, Some(TEXT_PLAIN)),
            ("text/html", Some(TEXT_HTML)),
            (SENSITIVE_MARKER, None),
            ("image/png", None),
        ];
        for (name, expected) in cases {
            // Act
            let item = item_mime_of(name);

            // Assert
            assert_eq!(item, expected, "{name}");
        }
    }
}
