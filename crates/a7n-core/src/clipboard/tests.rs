// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn generation(counter: u64, origin: u8) -> Generation {
    Generation {
        generation: counter,
        origin: MachineId::try_from(&[origin; 16][..]).unwrap(),
    }
}

fn item(mime: &str, size: usize) -> Item {
    Item {
        mime: mime.into(),
        data: vec![b'x'; size],
    }
}

#[test]
fn accept_generation_and_origin_order_requires_strict_increase() {
    // Arrange
    let current = generation(10, 2);
    for (incoming, expected) in [
        (generation(9, 3), false),
        (generation(10, 1), false),
        (generation(10, 2), false),
        (generation(10, 3), true),
        (generation(11, 1), true),
    ] {
        let wire = announce(vec![item(TEXT_PLAIN, 1)], incoming).unwrap();

        // Act
        let result = accept(Some(current), &wire).unwrap();

        // Assert
        assert_eq!(result.is_some(), expected);
    }
}

#[test]
fn next_observed_counter_advances_and_saturates() {
    // Arrange
    let mut clock = Clock::default();
    let own = generation(0, 1).origin;
    clock.observe(&generation(42, 2));
    clock.observe(&generation(10, 3));

    // Act
    let next = clock.next(own);
    clock.observe(&generation(u64::MAX, 2));
    let exhausted = clock.next(own);

    // Assert
    assert_eq!(next, generation(43, 1));
    assert_eq!(exhausted, generation(u64::MAX, 1));
}

#[test]
fn announce_inline_budget_keeps_other_items_pending() {
    // Arrange
    let items = vec![
        item(TEXT_PLAIN, limits::CLIPBOARD_INLINE_MAX),
        item(TEXT_HTML, 1),
    ];

    // Act
    let wire = announce(items, generation(1, 1)).unwrap();
    let received = accept(None, &wire).unwrap().unwrap();

    // Assert
    assert_eq!(
        wire.items[0].inline_data.len(),
        limits::CLIPBOARD_INLINE_MAX
    );
    assert_eq!(wire.items[1].inline_data, Vec::<u8>::new());
    assert_eq!(received.items.len(), 1);
    assert_eq!(received.pending, 1);
}

#[test]
fn announce_oversized_text_rejects_entire_copy() {
    // Arrange
    let items = vec![
        item(TEXT_PLAIN, 1),
        item(TEXT_HTML, limits::CLIPBOARD_TEXT_MAX + 1),
    ];

    // Act
    let result = announce(items, generation(1, 1));

    // Assert
    assert!(result.is_none());
}

#[test]
fn announce_empty_or_unsupported_items_are_dropped() {
    // Arrange
    let items = vec![item(TEXT_PLAIN, 0), item("image/png", 1)];

    // Act
    let result = announce(items, generation(1, 1));

    // Assert
    assert!(result.is_none());
}

#[test]
fn announce_text_at_limit_remains_metadata() {
    // Arrange
    let items = vec![
        item(TEXT_PLAIN, limits::CLIPBOARD_TEXT_MAX),
        item(TEXT_HTML, 1),
    ];

    // Act
    let wire = announce(items, generation(1, 1)).unwrap();
    let received = accept(None, &wire).unwrap().unwrap();

    // Assert
    assert_eq!(wire.items[0].inline_data, Vec::<u8>::new());
    assert_eq!(wire.items[1].inline_data.len(), 1);
    assert_eq!(received.pending, 1);
}

#[test]
fn allows_sensitive_or_denied_app_rejects_change() {
    // Arrange
    let denied = vec!["Password.App".into()];
    for (sensitive, app, expected) in [
        (true, None, false),
        (false, Some("PASSWORD.APP"), false),
        (false, Some("other.app"), true),
        (false, None, true),
    ] {
        let change = LocalChange {
            items: vec![],
            sensitive,
            source_app: app.map(str::to_owned),
        };

        // Act
        let allowed = Exclusion::allows(&change, &denied);

        // Assert
        assert_eq!(allowed, expected);
    }
}

#[test]
fn accept_invalid_payload_rejects_without_applying_items() {
    // Arrange
    let valid = announce(vec![item(TEXT_PLAIN, 2)], generation(1, 1)).unwrap();
    for fault in [
        "origin",
        "size",
        "length",
        "hash",
        "hash_length",
        "inline_total",
    ] {
        let mut wire = valid.clone();
        let expected = match fault {
            "origin" => {
                wire.origin.pop();
                ClipboardError::Origin
            }
            "size" => {
                wire.items[0].size = limits::CLIPBOARD_TEXT_MAX as u64 + 1;
                ClipboardError::Size
            }
            "length" => {
                wire.items[0].size += 1;
                ClipboardError::Length
            }
            "hash" => {
                wire.items[0].sha256[0] ^= 1;
                ClipboardError::Digest
            }
            "hash_length" => {
                wire.items[0].sha256.pop();
                ClipboardError::Digest
            }
            _ => {
                wire = announce(
                    vec![item(TEXT_PLAIN, limits::CLIPBOARD_INLINE_MAX)],
                    generation(1, 1),
                )
                .unwrap();
                wire.items.push(wire.items[0].clone());
                ClipboardError::InlineLimit
            }
        };

        // Act
        let result = accept(None, &wire);

        // Assert
        assert_eq!(result.unwrap_err(), expected);
    }
}
