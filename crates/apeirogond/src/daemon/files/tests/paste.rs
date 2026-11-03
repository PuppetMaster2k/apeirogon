// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_core::clipboard::{FILES, Item, LocalChange, TEXT_PLAIN};
use a7n_platform::clipboard::{Clipboard, ClipboardCommand, LazyItem, PasteRequest, uri_list};
use tokio::sync::{mpsc, oneshot};

use super::*;

struct Fake {
    changes: mpsc::Sender<LocalChange>,
    commands: mpsc::Receiver<ClipboardCommand>,
    pastes: mpsc::Sender<PasteRequest>,
}

fn with_clipboard(daemon: Daemon) -> (Daemon, Fake) {
    let (changes, change_rx) = mpsc::channel(8);
    let (command_tx, commands) = mpsc::channel(8);
    let (pastes, paste_rx) = mpsc::channel(8);
    let daemon = daemon.with_clipboard(Some(Clipboard {
        images: false,
        files: true,
        changes: change_rx,
        commands: command_tx,
        pastes: paste_rx,
    }));
    (
        daemon,
        Fake {
            changes,
            commands,
            pastes,
        },
    )
}

fn copied_files(dir: &std::path::Path, items: Vec<Item>) -> LocalChange {
    let source = dir.join("copied");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("one.txt"), b"first").unwrap();
    fs::write(
        source.join("two.bin"),
        vec![7; a7n_proto::limits::CHUNK_MAX + 1],
    )
    .unwrap();
    LocalChange {
        items,
        files: vec![source],
        sensitive: false,
        source_app: None,
    }
}

async fn next_set(fake: &mut Fake) -> (a7n_core::clipboard::Generation, Vec<Item>, Vec<LazyItem>) {
    let ClipboardCommand::Set {
        generation,
        items,
        lazy,
    } = timeout(TEST_TIMEOUT, fake.commands.recv())
        .await
        .unwrap()
        .unwrap();
    (generation, items, lazy)
}

#[tokio::test]
async fn paste_files_copied_on_peer_downloads_and_replies_with_uri_list() {
    // Arrange
    let (a, a_dir) = daemon();
    let (b, b_dir) = daemon();
    let ((a, a_fake), (b, mut b_fake)) = (with_clipboard(a), with_clipboard(b));
    let receive_dir = b.config.files.receive_dir().unwrap();
    let change = copied_files(a_dir.path(), vec![]);
    let (a, b) = (Peer::run(a, a_dir), Peer::run(b, b_dir));
    a.pair(&b).await;
    a_fake.changes.send(change).await.unwrap();
    let (generation, items, lazy) = next_set(&mut b_fake).await;
    let (reply, receiver) = oneshot::channel();

    // Act
    b_fake
        .pastes
        .send(PasteRequest {
            generation,
            item_index: lazy[0].item_index,
            reply,
        })
        .await
        .unwrap();
    let list = timeout(TEST_TIMEOUT, receiver).await.unwrap().unwrap();

    // Assert
    assert_eq!(items, []);
    assert_eq!(lazy[0].mime, FILES);
    let paths = uri_list::decode(&list.unwrap());
    assert_eq!(paths, [receive_dir.join("copied")]);
    assert_eq!(fs::read(paths[0].join("one.txt")).unwrap(), b"first");
    assert_eq!(
        fs::read(paths[0].join("two.bin")).unwrap().len(),
        a7n_proto::limits::CHUNK_MAX + 1
    );
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn receive_clipboard_files_without_receive_policy_is_not_offered() {
    for policy in ["off", "send"] {
        // Arrange
        let (a, a_dir) = daemon();
        let (mut b, b_dir) = daemon();
        b.config
            .set(&format!("peers.{}.files", a.identity.machine_id), policy)
            .unwrap();
        let ((a, a_fake), (b, mut b_fake)) = (with_clipboard(a), with_clipboard(b));
        let text = Item {
            mime: TEXT_PLAIN.into(),
            data: b"paths".to_vec(),
        };
        let change = copied_files(a_dir.path(), vec![text.clone()]);
        let (a, b) = (Peer::run(a, a_dir), Peer::run(b, b_dir));
        a.pair(&b).await;

        // Act
        a_fake.changes.send(change).await.unwrap();
        let (_, items, lazy) = next_set(&mut b_fake).await;

        // Assert
        assert_eq!(items, [text], "{policy}");
        assert_eq!(lazy, [], "{policy}");
        a.stop().await;
        b.stop().await;
    }
}
