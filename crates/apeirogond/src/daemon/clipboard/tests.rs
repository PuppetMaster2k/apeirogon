// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::{Ipv4Addr, SocketAddr};

use a7n_core::clipboard::{Item, TEXT_PLAIN};
use a7n_proto::v1::Envelope;
use tempfile::TempDir;
use tokio::sync::mpsc;
use tokio::time::timeout;

use super::*;
use crate::daemon::test_peer::{Peer, TEST_TIMEOUT};

fn daemon() -> (
    Daemon,
    TempDir,
    mpsc::Sender<LocalChange>,
    mpsc::Receiver<ClipboardCommand>,
) {
    let dir = tempfile::tempdir().unwrap();
    let daemon = Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    let (changes, change_rx) = mpsc::channel(8);
    let (command_tx, commands) = mpsc::channel(8);
    (
        daemon.with_clipboard(Some(Clipboard {
            changes: change_rx,
            commands: command_tx,
            pastes: mpsc::channel(1).1,
        })),
        dir,
        changes,
        commands,
    )
}

fn change(text: &str) -> LocalChange {
    LocalChange {
        items: vec![Item {
            mime: TEXT_PLAIN.into(),
            data: text.as_bytes().to_vec(),
        }],
        sensitive: false,
        source_app: None,
    }
}

fn wire(origin: MachineId, generation: u64, text: &str) -> ClipboardAnnounce {
    clipboard::announce(change(text).items, Generation { origin, generation }).unwrap()
}

mod fetch;

#[tokio::test]
async fn clipboard_changed_two_daemons_delivers_only_non_sensitive_copy() {
    // Arrange
    let (a, a_dir, changes, _a_commands) = daemon();
    let (b, b_dir, _b_changes, mut commands) = daemon();
    let (a, b) = (Peer::run(a, a_dir), Peer::run(b, b_dir));
    a.pair(&b).await;
    let mut sensitive = change("secret");
    sensitive.sensitive = true;

    // Act
    changes.send(sensitive).await.unwrap();
    changes.send(change("public")).await.unwrap();
    let command = timeout(TEST_TIMEOUT, commands.recv())
        .await
        .unwrap()
        .unwrap();

    // Assert
    let ClipboardCommand::Set {
        generation, items, ..
    } = command;
    assert_eq!(
        generation,
        Generation {
            generation: 1,
            origin: a.machine
        }
    );
    assert_eq!(items, change("public").items);
    assert!(commands.try_recv().is_err());
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn clipboard_link_up_own_copy_is_sent_to_new_peer() {
    // Arrange
    let (mut a, a_dir, _changes, _a_commands) = daemon();
    let (b, b_dir, _b_changes, mut commands) = daemon();
    a.clipboard_changed(Some(change("before pairing")));
    let (a, b) = (Peer::run(a, a_dir), Peer::run(b, b_dir));

    // Act
    a.pair(&b).await;
    let command = timeout(TEST_TIMEOUT, commands.recv())
        .await
        .unwrap()
        .unwrap();

    // Assert
    let ClipboardCommand::Set { items, .. } = command;
    assert_eq!(items, change("before pairing").items);
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn receive_clipboard_send_policy_ignores_copy_and_observes_counter() {
    // Arrange
    let (mut a, _a_dir, _a_changes, _a_commands) = daemon();
    let (mut b, _b_dir, _b_changes, mut commands) = daemon();
    let peer = a.identity.machine_id;
    b.config
        .set(&format!("peers.{peer}.clipboard"), "send")
        .unwrap();
    a.clipboard_changed(Some(change("ignored")));
    let incoming = a.clipboard.announce.clone().unwrap();

    // Act
    b.receive_from(
        peer,
        Envelope {
            body: Some(Body::ClipboardAnnounce(incoming)),
        },
    )
    .await
    .unwrap();

    // Assert
    assert!(commands.try_recv().is_err());
    assert!(b.clipboard.current.is_none());
    assert_eq!(b.clipboard.clock.next(b.identity.machine_id).generation, 2);
}

#[tokio::test]
async fn receive_clipboard_old_or_invalid_copy_keeps_current_and_backend_alive() {
    // Arrange
    let (mut daemon, _dir, _changes, mut commands) = daemon();
    let peer = daemon.identity.machine_id;
    let initial = wire(peer, 10, "current");
    daemon.receive_clipboard(peer, &initial);
    commands.try_recv().unwrap();
    let mut invalid = wire(peer, 20, "invalid");
    invalid.items[0].sha256[0] ^= 1;

    // Act
    daemon
        .receive_from(
            peer,
            Envelope {
                body: Some(Body::ClipboardAnnounce(invalid)),
            },
        )
        .await
        .unwrap();
    daemon.receive_clipboard(peer, &wire(peer, 9, "older"));

    // Assert
    assert!(commands.try_recv().is_err());
    assert_eq!(daemon.clipboard.current.unwrap().generation, 10);
    assert!(daemon.clipboard.handle.is_some());
    assert_eq!(daemon.clipboard.clock.next(peer).generation, 21);
    daemon.receive_clipboard(peer, &wire(peer, 21, "next"));
    assert!(commands.try_recv().is_ok());
}

#[tokio::test]
async fn receive_clipboard_metadata_only_installs_lazy_item() {
    // Arrange
    let (mut daemon, _dir, _changes, mut commands) = daemon();
    let peer = daemon.identity.machine_id;
    let incoming = wire(
        peer,
        10,
        &"x".repeat(a7n_proto::limits::CLIPBOARD_INLINE_MAX + 1),
    );

    // Act
    daemon.receive_clipboard(peer, &incoming);
    daemon.receive_clipboard(peer, &wire(peer, 9, "older"));

    // Assert
    let ClipboardCommand::Set { items, lazy, .. } = commands.try_recv().unwrap();
    assert_eq!(items, []);
    assert_eq!(lazy.len(), 1);
    assert_eq!(lazy[0].item_index, 0);
    assert_eq!(daemon.clipboard.current.unwrap().generation, 10);
    assert_eq!(daemon.clipboard.announce, Some(incoming));
}

#[tokio::test]
async fn clipboard_changed_deny_list_does_not_advance_or_replay_previous_copy() {
    // Arrange
    let (mut daemon, _dir, _changes, _commands) = daemon();
    daemon.config.clipboard.deny_apps = vec!["secret.app".into()];
    daemon.clipboard_changed(Some(change("previous")));
    let mut denied = change("secret");
    denied.source_app = Some("SECRET.APP".into());

    // Act
    daemon.clipboard_changed(Some(denied));

    // Assert
    assert_eq!(daemon.clipboard.current.unwrap().generation, 1);
    assert!(daemon.clipboard.announce.is_none());
    assert_eq!(
        daemon
            .clipboard
            .clock
            .next(daemon.identity.machine_id)
            .generation,
        2
    );
}

#[tokio::test]
async fn clipboard_link_up_send_policy_or_older_generation_does_not_set_backend() {
    for send_only in [true, false] {
        // Arrange
        let (mut a, a_dir, _a_changes, _a_commands) = daemon();
        let (mut b, b_dir, _b_changes, mut commands) = daemon();
        a.clipboard_changed(Some(change("A copy")));
        if send_only {
            b.config
                .set(
                    &format!("peers.{}.clipboard", a.identity.machine_id),
                    "send",
                )
                .unwrap();
        } else {
            b.clipboard.clock.observe(&Generation {
                generation: 10,
                origin: a.identity.machine_id,
            });
            b.clipboard_changed(Some(change("newer B copy")));
        }
        let (a, b) = (Peer::run(a, a_dir), Peer::run(b, b_dir));

        // Act
        a.pair(&b).await;
        // Layout follows the link-up announce on the same reliable control stream.
        a.command(|reply| crate::command::Command::SetLayout {
            placed: vec![a7n_ipc::LayoutPlacement {
                machine: a.machine,
                x: 0,
                y: 0,
                width: 100,
                height: 100,
            }],
            reply,
        })
        .await
        .unwrap();
        timeout(TEST_TIMEOUT, async {
            loop {
                let layout = b.command(crate::command::Command::GetLayout).await.unwrap();
                if !layout.placed.is_empty() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        // Assert
        assert!(commands.try_recv().is_err());
        a.stop().await;
        b.stop().await;
    }
}
