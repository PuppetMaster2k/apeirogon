// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::net::{Ipv4Addr, SocketAddr};

use a7n_proto::v1::{FileChunk, transfer_frame::Frame};
use tempfile::TempDir;
use tokio::time::timeout;

use super::*;
use crate::command::Command;
use crate::daemon::test_peer::{Peer, TEST_TIMEOUT};

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
