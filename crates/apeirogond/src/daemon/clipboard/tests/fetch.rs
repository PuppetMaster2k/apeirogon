// SPDX-License-Identifier: GPL-3.0-or-later

use tokio::sync::oneshot;

use super::*;

fn with_pastes(mut daemon: Daemon) -> (Daemon, mpsc::Sender<PasteRequest>) {
    let (sender, receiver) = mpsc::channel(8);
    daemon.clipboard.handle.as_mut().unwrap().pastes = receiver;
    (daemon, sender)
}

#[tokio::test]
async fn clipboard_paste_large_copy_with_both_or_ask_fetches_source_bytes() {
    for ask in [false, true] {
        // Arrange
        let (mut a, a_dir, _changes, _a_commands) = daemon();
        let (mut b, b_dir, _b_changes, mut commands) = daemon();
        if ask {
            a.config
                .set(&format!("peers.{}.clipboard", b.identity.machine_id), "ask")
                .unwrap();
            b.config
                .set(&format!("peers.{}.clipboard", a.identity.machine_id), "ask")
                .unwrap();
        }
        let (b, pastes) = with_pastes(b);
        let text = "x".repeat(100 * 1024);
        a.clipboard_changed(Some(change(&text)));
        let (a, b) = (Peer::run(a, a_dir), Peer::run(b, b_dir));
        a.pair(&b).await;
        if ask {
            let held = b
                .wait(|s| s.held_clipboard.is_some())
                .await
                .held_clipboard
                .unwrap();
            assert!(commands.try_recv().is_err());
            b.command(|reply| crate::command::Command::ClipboardAccept {
                machine: held.machine,
                generation: held.generation,
                reply,
            })
            .await
            .unwrap();
        }
        let ClipboardCommand::Set {
            generation,
            items,
            lazy,
        } = timeout(TEST_TIMEOUT, commands.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(items, []);
        assert_eq!(
            lazy,
            vec![LazyItem {
                mime: TEXT_PLAIN.into(),
                size: text.len() as u64,
                item_index: 0,
                top_level: 0,
            }]
        );
        let (reply, receiver) = oneshot::channel();

        // Act
        pastes
            .send(PasteRequest {
                generation,
                item_index: 0,
                reply,
            })
            .await
            .unwrap();
        let data = timeout(a7n_proto::limits::HANDSHAKE_TIMEOUT, receiver)
            .await
            .unwrap()
            .unwrap();

        // Assert
        assert_eq!(data, Some(text.into_bytes()));
        a.stop().await;
        b.stop().await;
    }
}

#[tokio::test]
async fn clipboard_paste_stale_source_or_receive_only_source_returns_none() {
    for stale in [true, false] {
        // Arrange
        let (mut a, a_dir, _changes, _a_commands) = daemon();
        let (mut b, b_dir, _b_changes, mut commands) = daemon();
        a.clipboard_changed(Some(change(&"x".repeat(100 * 1024))));
        b.receive_clipboard(
            a.identity.machine_id,
            a.clipboard.announce.as_ref().unwrap(),
        );
        let ClipboardCommand::Set { generation, .. } = commands.recv().await.unwrap();
        if stale {
            let mut newer = change("new copy");
            newer.sensitive = true;
            a.clipboard_changed(Some(newer));
        } else {
            a.config
                .set(
                    &format!("peers.{}.clipboard", b.identity.machine_id),
                    "receive",
                )
                .unwrap();
        }
        b.config
            .set(
                &format!("peers.{}.clipboard", a.identity.machine_id),
                "receive",
            )
            .unwrap();
        let (b, pastes) = with_pastes(b);
        let (a, b) = (Peer::run(a, a_dir), Peer::run(b, b_dir));
        a.pair(&b).await;
        let (reply, receiver) = oneshot::channel();

        // Act
        pastes
            .send(PasteRequest {
                generation,
                item_index: 0,
                reply,
            })
            .await
            .unwrap();
        let data = timeout(a7n_proto::limits::HANDSHAKE_TIMEOUT, receiver)
            .await
            .unwrap()
            .unwrap();

        // Assert
        assert_eq!(data, None);
        a.stop().await;
        b.stop().await;
    }
}

#[tokio::test]
async fn clipboard_paste_peer_without_image_capability_fetches_correct_text_item() {
    // Arrange
    let (mut a, a_dir, _changes, _a_commands) = daemon();
    let (mut b, b_dir, _b_changes, mut commands) = daemon();
    b.info
        .capabilities
        .retain(|&capability| capability != i32::from(Capability::ClipboardImage));
    let (b, pastes) = with_pastes(b);
    let text = "x".repeat(limits::CLIPBOARD_INLINE_MAX + 1);
    let mut copy = change(&text);
    copy.items.insert(
        0,
        Item {
            mime: clipboard::IMAGE_PNG.into(),
            data: vec![0; limits::CLIPBOARD_INLINE_MAX + 1],
        },
    );
    a.clipboard_changed(Some(copy));
    let (a, b) = (Peer::run(a, a_dir), Peer::run(b, b_dir));
    a.pair(&b).await;
    let ClipboardCommand::Set {
        generation,
        items,
        lazy,
    } = timeout(TEST_TIMEOUT, commands.recv())
        .await
        .unwrap()
        .unwrap();
    let (reply, receiver) = oneshot::channel();

    // Act
    pastes
        .send(PasteRequest {
            generation,
            item_index: 0,
            reply,
        })
        .await
        .unwrap();
    let data = timeout(TEST_TIMEOUT, receiver).await.unwrap().unwrap();

    // Assert
    assert_eq!(items, []);
    assert_eq!(lazy.len(), 1);
    assert_eq!(lazy[0].mime, TEXT_PLAIN);
    assert_eq!(lazy[0].item_index, 0);
    assert_eq!(data, Some(text.into_bytes()));
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn clipboard_paste_invalid_fetched_png_fails_paste() {
    // Arrange
    let (mut a, a_dir, _changes, _a_commands) = daemon();
    let (b, b_dir, _b_changes, mut commands) = daemon();
    let (b, pastes) = with_pastes(b);
    a.clipboard_changed(Some(LocalChange {
        files: Vec::new(),
        items: vec![Item {
            mime: clipboard::IMAGE_PNG.into(),
            data: vec![0; limits::CLIPBOARD_INLINE_MAX + 1],
        }],
        sensitive: false,
        source_app: None,
    }));
    let (a, b) = (Peer::run(a, a_dir), Peer::run(b, b_dir));
    a.pair(&b).await;
    let ClipboardCommand::Set { generation, .. } = timeout(TEST_TIMEOUT, commands.recv())
        .await
        .unwrap()
        .unwrap();
    let (reply, receiver) = oneshot::channel();

    // Act
    pastes
        .send(PasteRequest {
            generation,
            item_index: 0,
            reply,
        })
        .await
        .unwrap();
    let data = timeout(TEST_TIMEOUT, receiver).await.unwrap().unwrap();

    // Assert
    assert_eq!(data, None);
    a.stop().await;
    b.stop().await;
}
