// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn verify_transfer_corrupt_or_out_of_order_frames_are_rejected() {
    // Arrange
    let item = ClipboardItem {
        size: 3,
        sha256: Sha256::digest(b"abc").to_vec(),
        ..ClipboardItem::default()
    };
    let cases = [
        vec![Frame::Chunk(FileChunk {
            file_index: 0,
            offset: 0,
            data: b"abcd".to_vec(),
        })],
        vec![Frame::Chunk(FileChunk {
            file_index: 0,
            offset: 1,
            data: b"abc".to_vec(),
        })],
        vec![Frame::Chunk(FileChunk {
            file_index: 1,
            offset: 0,
            data: b"abc".to_vec(),
        })],
        vec![Frame::Done(TransferDone {})],
        vec![Frame::FileEnd(FileEnd {
            file_index: 0,
            sha256: item.sha256.clone(),
        })],
        vec![
            Frame::Chunk(FileChunk {
                file_index: 0,
                offset: 0,
                data: b"bad".to_vec(),
            }),
            Frame::FileEnd(FileEnd {
                file_index: 0,
                sha256: item.sha256.clone(),
            }),
        ],
        vec![
            Frame::Chunk(FileChunk {
                file_index: 0,
                offset: 0,
                data: b"abc".to_vec(),
            }),
            Frame::FileEnd(FileEnd {
                file_index: 0,
                sha256: vec![0; 32],
            }),
        ],
    ];
    for frames in cases {
        let mut verifier = Verifier::new(&item).unwrap();

        // Act
        let result: Result<Vec<_>, _> = frames
            .into_iter()
            .map(|frame| verifier.apply(frame))
            .collect();

        // Assert
        assert!(result.is_err());
    }
}
