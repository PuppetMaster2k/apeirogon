// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::net::{Ipv4Addr, SocketAddr};

use a7n_proto::v1::{Capability, FileChunk, transfer_frame::Frame};
use tempfile::TempDir;
use tokio::time::timeout;

use super::*;
use crate::command::Command;
use crate::daemon::test_peer::{Peer, TEST_TIMEOUT};

mod paste;
mod resume;
mod space;

fn tempdir() -> TempDir {
    tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap()
}

fn daemon() -> (Daemon, TempDir) {
    let dir = tempdir();
    let mut daemon = Daemon::load(dir.path(), SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
    daemon.config.files.receive_dir = Some(dir.path().join("received"));
    daemon.config.notifications.files = false;
    (daemon, dir)
}

async fn offer(a: &Peer, b: &Peer, path: PathBuf) -> u64 {
    a.command(|reply| Command::SendFiles {
        peer: b.machine,
        paths: vec![path],
        reply,
    })
    .await
    .unwrap()
    .0
}

async fn landed(peer: &Peer, path: &std::path::Path) {
    timeout(TEST_TIMEOUT, async {
        loop {
            if path.exists() {
                return;
            }
            peer.status().await;
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn offer_files_receive_policy_verifies_tree_and_preserves_collisions() {
    // Arrange
    let (a, a_dir) = daemon();
    let (mut b, b_dir) = daemon();
    b.config
        .set(&format!("peers.{}.files", a.identity.machine_id), "receive")
        .unwrap();
    let source = a_dir.path().join("tree");
    fs::create_dir(&source).unwrap();
    let data = vec![42; a7n_proto::limits::CHUNK_MAX + 19];
    fs::write(source.join("data.bin"), &data).unwrap();
    fs::write(source.join("empty"), []).unwrap();
    fs::create_dir(source.join("directory")).unwrap();
    let receive_dir = b.config.files.receive_dir().unwrap();
    fs::create_dir_all(receive_dir.join("tree")).unwrap();
    fs::write(receive_dir.join("tree/original"), b"original").unwrap();
    let (a, b) = (Peer::run(a, a_dir), Peer::run(b, b_dir));
    a.pair(&b).await;

    // Act
    let offer_id = offer(&a, &b, source).await;
    landed(&b, &receive_dir.join("tree (2)")).await;
    let sender = a.wait(|status| !status.finished_sends.is_empty()).await;

    // Assert
    assert_eq!(sender.finished_sends[0].offer_id, offer_id);
    assert_eq!(
        sender.finished_sends[0].outcome,
        a7n_ipc::SendOutcome::Completed
    );
    assert_eq!(
        fs::read(receive_dir.join("tree (2)/data.bin")).unwrap(),
        data
    );
    assert!(receive_dir.join("tree (2)/directory").is_dir());
    assert_eq!(
        fs::read(receive_dir.join("tree (2)/empty")).unwrap(),
        [] as [u8; 0]
    );
    assert_eq!(
        fs::read(receive_dir.join("tree/original")).unwrap(),
        b"original"
    );
    assert!(b.status().await.held_clipboard.is_none());
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn offer_files_peer_without_capability_is_refused() {
    // Arrange
    let (a, a_dir) = daemon();
    let (mut b, b_dir) = daemon();
    b.info
        .capabilities
        .retain(|value| *value != i32::from(Capability::Files));
    let source = a_dir.path().join("send.txt");
    fs::write(&source, b"payload").unwrap();
    let (a, b) = (Peer::run(a, a_dir), Peer::run(b, b_dir));
    a.pair(&b).await;

    // Act
    let result = a
        .command(|reply| Command::SendFiles {
            peer: b.machine,
            paths: vec![source],
            reply,
        })
        .await;

    // Assert
    assert!(matches!(result, Err(DaemonError::InvalidRequest(_))));
    assert_eq!(b.status().await.held_files, []);
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn receive_files_tampered_content_removes_staging_without_publishing() {
    // Arrange
    let (a, a_dir) = daemon();
    let (b, b_dir) = daemon();
    let source = a_dir.path().join("send.txt");
    fs::write(&source, b"original").unwrap();
    let receive_dir = b.config.files.receive_dir().unwrap();
    let (a, b) = (Peer::run(a, a_dir), Peer::run(b, b_dir));
    a.pair(&b).await;
    offer(&a, &b, source.clone()).await;
    let status = b.wait(|status| !status.held_files.is_empty()).await;
    fs::write(source, b"tampered").unwrap();

    // Act
    b.command(|reply| Command::FilesAccept {
        peer: a.machine,
        offer_id: status.held_files[0].offer_id,
        reply,
    })
    .await
    .unwrap();
    b.wait(|status| status.transfers.is_empty()).await;

    // Assert
    assert!(!receive_dir.join("send.txt").exists());
    assert_eq!(
        fs::read_dir(receive_dir.join(".apeirogon-partial"))
            .unwrap()
            .count(),
        0
    );
    a.stop().await;
    b.stop().await;
}

#[test]
fn apply_chunk_invalid_offsets_or_size_are_rejected() {
    for (index, offset, data) in [(0, 1, vec![1]), (0, 0, vec![1; 8]), (1, 0, vec![1])] {
        // Arrange
        let dir = tempdir();
        let source = dir.path().join("source");
        fs::write(&source, b"payload").unwrap();
        let origin = MachineId::try_from([1; MachineId::LEN].as_slice()).unwrap();
        let offer = offer::build(origin, 1, vec![source]).unwrap();
        let manifest = ValidManifest::try_from(offer.wire.files.as_ref().unwrap()).unwrap();
        let mut staging = staging::prepare(&dir.path().join("stage"), &manifest).unwrap();
        let progress = Progress::new(&manifest, 7);

        // Act
        let result = receive::apply(
            &mut staging,
            &manifest,
            Frame::Chunk(FileChunk {
                file_index: index,
                offset,
                data,
            }),
            &progress,
        );

        // Assert
        assert_eq!(result, Err(Failure::Invalid));
    }
}

#[cfg(unix)]
#[test]
fn build_source_tree_symlinks_are_skipped_without_reading_target() {
    // Arrange
    let dir = tempdir();
    let source = dir.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("regular"), b"safe").unwrap();
    std::os::unix::fs::symlink("/does/not/exist", source.join("link")).unwrap();
    std::os::unix::fs::symlink(dir.path(), source.join("cycle")).unwrap();
    let origin = MachineId::try_from([1; MachineId::LEN].as_slice()).unwrap();

    // Act
    let offer = offer::build(origin, 1, vec![source]).unwrap();

    // Assert
    assert_eq!(offer.skipped, 2);
    assert_eq!(offer.wire.files.unwrap().entries.len(), 2);
}

#[test]
fn prepare_complete_staged_file_resumes_only_when_hash_matches() {
    // Arrange
    let cases: [(&[u8], u64); 2] = [(b"payload", 7), (b"corrupt", 0)];
    for (staged, expected_offset) in cases {
        let temp = tempdir();
        let source = temp.path().join("source");
        fs::write(&source, b"payload").unwrap();
        let origin = MachineId::try_from([1; MachineId::LEN].as_slice()).unwrap();
        let offer = offer::build(origin, 1, vec![source]).unwrap();
        let manifest = ValidManifest::try_from(offer.wire.files.as_ref().unwrap()).unwrap();
        let stage = temp.path().join("stage");
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("source"), staged).unwrap();

        // Act
        let prepared = staging::prepare(&stage, &manifest).unwrap();

        // Assert
        assert_eq!(prepared.files[0].as_ref().unwrap().written, expected_offset);
        assert_eq!(prepared.resume.len(), usize::from(expected_offset > 0));
        if expected_offset > 0 {
            assert_eq!(prepared.resume[0].offset, expected_offset);
        }
    }
}

#[cfg(unix)]
#[test]
fn cleanup_old_staging_removes_only_expired_directories() {
    // Arrange
    let temp = tempdir();
    let parent = temp.path().join(".apeirogon-partial");
    fs::create_dir(&parent).unwrap();
    let stale = parent.join("stale");
    let fresh = parent.join("fresh");
    fs::create_dir(&stale).unwrap();
    fs::create_dir(&fresh).unwrap();
    fs::write(stale.join("file"), b"partial").unwrap();
    let now = std::time::SystemTime::now();
    let modified = now - PARTIAL_MAX_AGE - Duration::from_secs(1);
    fs::File::open(&stale)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();

    // Act
    staging::cleanup(temp.path(), now);

    // Assert
    assert!(!stale.exists());
    assert!(fresh.is_dir());
}

#[test]
fn build_decomposed_source_name_normalizes_wire_path() {
    // Arrange
    let temp = tempdir();
    let source = temp.path().join("cafe\u{301}.txt");
    fs::write(&source, b"payload").unwrap();
    let origin = MachineId::try_from([1; MachineId::LEN].as_slice()).unwrap();

    // Act
    let offer = offer::build(origin, 1, vec![source.clone()]).unwrap();

    // Assert
    assert_eq!(offer.wire.files.unwrap().entries[0].path, "caf\u{e9}.txt");
    assert_eq!(offer.paths, [source]);
}
