// SPDX-License-Identifier: GPL-3.0-or-later

//! Which offered formats to read from a foreign selection. Pure.

use super::super::formats::{SENSITIVE_MARKER, names_of};
use a7n_core::clipboard::{TEXT_HTML, TEXT_PLAIN};

/// Offered next to our own selection so the backend recognises its own write
/// coming back from the compositor and does not announce it again.
pub(super) const OWN_MARKER: &str = "application/x-apeirogon-clipboard";

/// One format to read: the item it becomes and the name it is offered under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Read {
    pub(super) item: &'static str,
    pub(super) offered: &'static str,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct ReadPlan {
    pub(super) sensitive: bool,
    pub(super) reads: Vec<Read>,
}

pub(super) fn is_own_selection(offered: &[String]) -> bool {
    offered.iter().any(|name| name == OWN_MARKER)
}

/// Picks the best offered name for each supported item. A sensitive marker
/// means nothing is read.
pub(super) fn plan_reads(offered: &[String]) -> ReadPlan {
    if offered.iter().any(|name| name == SENSITIVE_MARKER) {
        return ReadPlan {
            sensitive: true,
            reads: Vec::new(),
        };
    }
    let reads = [TEXT_PLAIN, TEXT_HTML]
        .into_iter()
        .filter_map(|item| {
            let offered = names_of(item)
                .iter()
                .find(|name| offered.iter().any(|have| have == *name))?;
            Some(Read { item, offered })
        })
        .collect();
    ReadPlan {
        sensitive: false,
        reads,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn plan_reads_prefers_charset_names_and_maps_supported_items() {
        // Arrange
        let cases = [
            (
                names(&["text/plain", "UTF8_STRING", TEXT_PLAIN, "text/html"]),
                vec![(TEXT_PLAIN, TEXT_PLAIN), (TEXT_HTML, "text/html")],
            ),
            (names(&["UTF8_STRING"]), vec![(TEXT_PLAIN, "UTF8_STRING")]),
            (names(&["image/png", "TARGETS"]), vec![]),
        ];
        for (offered, expected) in cases {
            // Act
            let plan = plan_reads(&offered);

            // Assert
            let reads: Vec<_> = plan.reads.iter().map(|r| (r.item, r.offered)).collect();
            assert_eq!(reads, expected);
            assert!(!plan.sensitive);
        }
    }

    #[test]
    fn plan_reads_sensitive_marker_offered_reads_nothing() {
        // Arrange
        let offered = names(&[TEXT_PLAIN, SENSITIVE_MARKER]);

        // Act
        let plan = plan_reads(&offered);

        // Assert
        assert!(plan.sensitive);
        assert!(plan.reads.is_empty());
    }

    #[test]
    fn is_own_selection_marker_offered_is_recognised() {
        // Arrange
        let ours = names(&[TEXT_PLAIN, OWN_MARKER]);
        let foreign = names(&[TEXT_PLAIN]);

        // Act
        let results = (is_own_selection(&ours), is_own_selection(&foreign));

        // Assert
        assert_eq!(results, (true, false));
    }
}
