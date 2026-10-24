// SPDX-License-Identifier: GPL-3.0-or-later

//! The selection this backend owns after a `Set`, and how a paste is answered. Pure.

use a7n_core::clipboard::{Generation, IMAGE_PNG, Item, TEXT_HTML, TEXT_PLAIN};
#[cfg(test)]
use a7n_proto::limits::CLIPBOARD_TEXT_MAX;

use super::super::formats::{item_mime_of, names_of};
use super::super::{ClipboardCommand, LazyItem};

#[derive(Debug)]
pub(super) struct Served {
    generation: Generation,
    items: Vec<Item>,
    lazy: Vec<LazyItem>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Serve<'a> {
    Inline(&'a [u8]),
    Lazy {
        generation: Generation,
        item_index: u32,
    },
    Refuse,
}

impl Served {
    pub(super) fn new(command: ClipboardCommand) -> Self {
        let ClipboardCommand::Set {
            generation,
            items,
            lazy,
        } = command;
        Self {
            generation,
            items: items
                .into_iter()
                .filter(|item| item.data.len() <= a7n_core::clipboard::limit_for(&item.mime))
                .collect(),
            lazy: lazy
                .into_iter()
                .filter(|item| item.size <= a7n_core::clipboard::limit_for(&item.mime) as u64)
                .collect(),
        }
    }

    /// Every name to advertise, best first within each item.
    pub(super) fn offered(&self) -> Vec<&'static str> {
        [TEXT_PLAIN, TEXT_HTML, IMAGE_PNG]
            .into_iter()
            .filter(|mime| self.has(mime))
            .flat_map(|mime| names_of(mime).iter().copied())
            .collect()
    }

    fn has(&self, mime: &str) -> bool {
        self.items.iter().any(|item| item.mime == mime)
            || self.lazy.iter().any(|item| item.mime == mime)
    }

    /// Data already here is served directly; the rest is fetched from the origin.
    pub(super) fn decide(&self, requested: &str) -> Serve<'_> {
        let Some(mime) = item_mime_of(requested) else {
            return Serve::Refuse;
        };
        if let Some(item) = self.items.iter().find(|item| item.mime == mime) {
            return Serve::Inline(&item.data);
        }
        match self.lazy.iter().find(|item| item.mime == mime) {
            Some(item) => Serve::Lazy {
                generation: self.generation,
                item_index: item.item_index,
            },
            None => Serve::Refuse,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a7n_proto::MachineId;

    fn generation() -> Generation {
        Generation {
            generation: 3,
            origin: MachineId::try_from(&[1u8; 16][..]).unwrap(),
        }
    }

    fn served(items: Vec<Item>, lazy: Vec<LazyItem>) -> Served {
        Served::new(ClipboardCommand::Set {
            generation: generation(),
            items,
            lazy,
        })
    }

    fn item(mime: &str, len: usize) -> Item {
        Item {
            mime: mime.to_owned(),
            data: vec![b'a'; len],
        }
    }

    #[test]
    fn decide_inline_and_lazy_items_picks_source_by_item() {
        // Arrange
        let lazy = LazyItem {
            mime: TEXT_HTML.to_owned(),
            size: 9,
            item_index: 1,
        };
        let served = served(vec![item(TEXT_PLAIN, 2)], vec![lazy]);

        // Act
        let plain = served.decide("UTF8_STRING");
        let html = served.decide("text/html");
        let other = served.decide("image/png");

        // Assert
        assert_eq!(plain, Serve::Inline(b"aa"));
        assert_eq!(
            html,
            Serve::Lazy {
                generation: generation(),
                item_index: 1
            }
        );
        assert_eq!(other, Serve::Refuse);
    }

    #[test]
    fn new_oversized_items_are_dropped_and_not_offered() {
        // Arrange
        let big = LazyItem {
            mime: TEXT_HTML.to_owned(),
            size: CLIPBOARD_TEXT_MAX as u64 + 1,
            item_index: 1,
        };

        // Act
        let served = served(vec![item(TEXT_PLAIN, CLIPBOARD_TEXT_MAX + 1)], vec![big]);

        // Assert
        assert_eq!(served.offered(), Vec::<&str>::new());
    }

    #[test]
    fn offered_lists_every_name_of_present_items_only() {
        // Arrange
        let served = served(vec![item(TEXT_HTML, 1)], vec![]);

        // Act
        let offered = served.offered();

        // Assert
        assert_eq!(offered, ["text/html;charset=utf-8", "text/html"]);
    }
}

#[cfg(test)]
mod image_tests {
    use super::*;
    use a7n_proto::{MachineId, limits};

    #[test]
    fn decide_image_above_text_limit_offers_lazy_png() {
        // Arrange
        let generation = Generation {
            generation: 1,
            origin: MachineId::try_from(&[1; 16][..]).unwrap(),
        };
        let served = Served::new(ClipboardCommand::Set {
            generation,
            items: vec![],
            lazy: vec![LazyItem {
                mime: IMAGE_PNG.into(),
                size: limits::CLIPBOARD_TEXT_MAX as u64 + 1,
                item_index: 2,
            }],
        });

        // Act
        let offered = served.offered();
        let decision = served.decide(IMAGE_PNG);

        // Assert
        assert_eq!(offered, [IMAGE_PNG]);
        assert_eq!(
            decision,
            Serve::Lazy {
                generation,
                item_index: 2
            }
        );
    }
}
