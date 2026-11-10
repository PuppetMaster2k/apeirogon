// SPDX-License-Identifier: GPL-3.0-or-later

//! Clipboard formats shared by the X11 and Wayland backends.

use std::borrow::Cow;

use a7n_core::clipboard::{FILES, IMAGE_PNG, TEXT_HTML, TEXT_PLAIN};

/// Password managers offer this format next to a secret; such a copy is never
/// announced, and the exclusion cannot be turned off.
pub(super) const SENSITIVE_MARKER: &str = "x-kde-passwordManagerHint";
pub(super) const HTML_ALIAS: &str = "text/html";
pub(super) const HTML_UTF8_ALIAS: &str = "text/html;charset=utf-8";

/// Names for plain text, best first. The legacy `UTF8_STRING` still shows up
/// from `XWayland` clients.
pub(super) const URI_LIST: &str = "text/uri-list";
pub(super) const GNOME_COPIED_FILES: &str = "x-special/gnome-copied-files";

const PLAIN_NAMES: [&str; 3] = [TEXT_PLAIN, "text/plain", "UTF8_STRING"];
const HTML_NAMES: [&str; 2] = [HTML_UTF8_ALIAS, HTML_ALIAS];
const FILES_NAMES: [&str; 2] = [URI_LIST, GNOME_COPIED_FILES];

/// Every name an item is offered under, best first.
pub(super) fn names_of(item_mime: &str) -> &'static [&'static str] {
    match item_mime {
        TEXT_PLAIN => &PLAIN_NAMES,
        IMAGE_PNG => &[IMAGE_PNG],
        TEXT_HTML => &HTML_NAMES,
        FILES => &FILES_NAMES,
        _ => &[],
    }
}

/// The item format a requested name stands for.
pub(super) fn item_mime_of(name: &str) -> Option<&'static str> {
    [TEXT_PLAIN, TEXT_HTML, IMAGE_PNG, FILES]
        .into_iter()
        .find(|item| names_of(item).contains(&name))
}

/// Converts a daemon paste reply (`text/uri-list`) to what the requested name expects.
pub(super) fn render_files<'a>(requested: &str, uri_list: &'a [u8]) -> Cow<'a, [u8]> {
    if requested == GNOME_COPIED_FILES {
        return Cow::Owned(super::uri_list::encode_gnome_copied(
            &super::uri_list::decode(uri_list),
        ));
    }
    Cow::Borrowed(uri_list)
}
