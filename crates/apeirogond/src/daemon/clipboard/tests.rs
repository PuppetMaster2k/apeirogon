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
            images: true,
            files: false,
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
        files: Vec::new(),
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

mod ask;
mod fetch;

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
async fn clipboard_changed_sensitive_or_denied_copy_does_not_advance_or_replay_previous_copy() {
    for (sensitive, source_app) in [(true, None), (false, Some("SECRET.APP"))] {
        // Arrange
        let (mut daemon, _dir, _changes, _commands) = daemon();
        daemon.config.clipboard.deny_apps = vec!["secret.app".into()];
        daemon.clipboard_changed(Some(change("previous")));
        let mut rejected = change("secret");
        rejected.sensitive = sensitive;
        rejected.source_app = source_app.map(Into::into);

        // Act
        daemon.clipboard_changed(Some(rejected));

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
}
