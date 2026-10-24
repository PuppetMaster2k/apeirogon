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
    assert_eq!(received.pending, vec![1]);
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
    let items = vec![item(TEXT_PLAIN, 0), item("image/jpeg", 1)];

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
    assert_eq!(received.pending, vec![0]);
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
            items: vec![item(IMAGE_PNG, 1)],
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

#[test]
fn validate_png_header_and_dimensions_enforces_image_bounds() {
    // Arrange
    let tiny = [
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 16, 84, 50, 254, 15,
        0, 2, 20, 1, 102, 4, 224, 222, 127, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];
    let mut signature = tiny.to_vec();
    signature[0] = 0;
    let mut chunk = tiny.to_vec();
    chunk[12] = 0;
    let mut cases = vec![
        (tiny.to_vec(), Ok((1, 1))),
        (signature, Err(ImageError::Header)),
        (chunk, Err(ImageError::Header)),
        (tiny[..32].to_vec(), Err(ImageError::Header)),
    ];
    for (width, height) in [
        (0, 1),
        (1, 0),
        (limits::IMAGE_MAX_DIMENSION + 1, 1),
        (1, limits::IMAGE_MAX_DIMENSION + 1),
        (limits::IMAGE_MAX_DIMENSION, limits::IMAGE_MAX_DIMENSION),
    ] {
        let mut header = tiny.to_vec();
        header[16..20].copy_from_slice(&width.to_be_bytes());
        header[20..24].copy_from_slice(&height.to_be_bytes());
        cases.push((header, Err(ImageError::Dimensions)));
    }
    for (data, expected) in cases {
        // Act
        let result = validate_png(&data);

        // Assert
        assert_eq!(result, expected);
    }
}

#[test]
fn announce_image_items_uses_image_limit_and_shared_inline_budget() {
    for size in [
        1,
        limits::CLIPBOARD_INLINE_MAX,
        limits::CLIPBOARD_TEXT_MAX + 1,
        limits::IMAGE_CLIPBOARD_MAX,
    ] {
        // Arrange
        let items = vec![item(IMAGE_PNG, size)];

        // Act
        let wire = announce(items, generation(1, 1)).unwrap();
        let received = accept(None, &wire).unwrap().unwrap();

        // Assert
        assert_eq!(wire.items[0].size, size as u64);
        if size <= limits::CLIPBOARD_INLINE_MAX {
            assert_eq!(received.items[0].data.len(), size);
            assert_eq!(received.pending, []);
        } else {
            assert_eq!(received.pending, [0]);
            assert_eq!(received.items, []);
        }
    }
}

#[test]
fn announce_image_above_limit_rejects_copy() {
    // Arrange
    let items = vec![item(IMAGE_PNG, limits::IMAGE_CLIPBOARD_MAX + 1)];

    // Act
    let result = announce(items, generation(1, 1));

    // Assert
    assert!(result.is_none());
}

#[test]
fn accept_image_above_limit_rejects_metadata() {
    // Arrange
    let mut wire = announce(vec![item(IMAGE_PNG, 1)], generation(1, 1)).unwrap();
    wire.items[0].size = limits::IMAGE_CLIPBOARD_MAX as u64 + 1;

    // Act
    let result = accept(None, &wire);

    // Assert
    assert_eq!(result.unwrap_err(), ClipboardError::Size);
}
