// SPDX-License-Identifier: GPL-3.0-or-later

//! Needs Xvfb; tests share CLIPBOARD and serialize their ownership changes.
#![cfg(all(unix, not(target_os = "macos")))]

#[path = "x11_clipboard/support.rs"]
mod support;

use a7n_core::clipboard::{Item, TEXT_HTML, TEXT_PLAIN};
use a7n_platform::clipboard::{LazyItem, start};

use support::{Client, LARGE_SIZE, SELECTION, generation, receive, set, wait_owned};

#[test]
#[ignore = "needs an X server"]
fn watch_external_text_reports_plain_html_and_source_app() {
    // Arrange
    let _selection = SELECTION.lock().unwrap();
    let mut clipboard = start().unwrap();
    let mut client = Client::new();
    client.own();

    // Act
    let change = client.change(&mut clipboard, b"copied text", false, false);

    // Assert
    assert!(!change.sensitive);
    assert_eq!(change.source_app.as_deref(), Some("ClipboardTest"));
    assert_eq!(
        change.items,
        [TEXT_PLAIN, TEXT_HTML].map(|mime| Item {
            mime: mime.into(),
            data: b"copied text".to_vec()
        })
    );
}

#[test]
#[ignore = "needs an X server"]
fn watch_password_manager_marker_reports_sensitive_without_reading_data() {
    // Arrange
    let _selection = SELECTION.lock().unwrap();
    let mut clipboard = start().unwrap();
    let mut client = Client::new();
    client.own();

    // Act
    let change = client.change(&mut clipboard, b"must not be read", true, false);

    // Assert
    assert!(change.sensitive);
    assert_eq!(change.items, []);
    assert_eq!(client.reads, 0);
}

#[test]
#[ignore = "needs an X server"]
fn set_inline_text_answers_utf8_without_echo() {
    // Arrange
    let _selection = SELECTION.lock().unwrap();
    let mut clipboard = start().unwrap();
    let client = Client::new();
    client.own();
    set(
        &clipboard,
        vec![Item {
            mime: TEXT_PLAIN.into(),
            data: b"remote text".to_vec(),
        }],
        vec![],
    );
    wait_owned(&client);

    // Act
    client.convert(client.atoms.UTF8_STRING);
    let (data, incremental) = client.read();

    // Assert
    assert_eq!(data, b"remote text");
    assert!(!incremental);
    assert!(clipboard.changes.try_recv().is_err());
}

#[test]
#[ignore = "needs an X server"]
fn set_lazy_text_requests_paste_and_delivers_reply() {
    // Arrange
    let _selection = SELECTION.lock().unwrap();
    let mut clipboard = start().unwrap();
    let client = Client::new();
    client.own();
    set(
        &clipboard,
        vec![],
        vec![LazyItem {
            mime: TEXT_PLAIN.into(),
            size: 9,
            item_index: 3,
        }],
    );
    wait_owned(&client);

    // Act
    client.convert(client.atoms.UTF8_STRING);
    let paste = receive(&client, &mut clipboard.pastes);
    let requested = (paste.generation, paste.item_index);
    paste.reply.send(Some(b"lazy text".to_vec())).unwrap();
    let (data, _) = client.read();

    // Assert
    assert_eq!(requested, (generation(), 3));
    assert_eq!(data, b"lazy text");
}

#[test]
#[ignore = "needs an X server"]
fn watch_large_incremental_text_reassembles_items() {
    // Arrange
    let _selection = SELECTION.lock().unwrap();
    let mut clipboard = start().unwrap();
    let mut client = Client::new();
    let data = vec![b'x'; LARGE_SIZE];
    client.own();

    // Act
    let change = client.change(&mut clipboard, &data, false, true);

    // Assert
    assert_eq!(change.items.len(), 2);
    assert!(change.items.iter().all(|item| item.data == data));
}

#[test]
#[ignore = "needs an X server"]
fn set_large_text_serves_incremental_chunks() {
    // Arrange
    let _selection = SELECTION.lock().unwrap();
    let clipboard = start().unwrap();
    let client = Client::new();
    let data = vec![b'y'; LARGE_SIZE];
    client.own();
    set(
        &clipboard,
        vec![Item {
            mime: TEXT_PLAIN.into(),
            data: data.clone(),
        }],
        vec![],
    );
    wait_owned(&client);

    // Act
    client.convert(client.atoms.UTF8_STRING);
    let (received, incremental) = client.read();

    // Assert
    assert!(incremental);
    assert_eq!(received, data);
}
