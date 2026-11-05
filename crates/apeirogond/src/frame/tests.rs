// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_proto::{ProtoError, v1::Envelope};

use super::*;

pub(crate) struct StalledStream {
    pub(crate) prefix: Option<Vec<u8>>,
    pub(crate) reads: usize,
}

impl FrameStream for StalledStream {
    async fn read_chunk(&mut self, max: usize) -> Result<Option<Chunk>, ReadError> {
        self.reads += 1;
        match self.prefix.take() {
            Some(prefix) => {
                assert!(prefix.len() <= max);
                Ok(Some(Chunk {
                    offset: 0,
                    bytes: prefix.into(),
                }))
            }
            None => std::future::pending().await,
        }
    }
}

#[tokio::test(start_paused = true)]
async fn read_oversized_or_stalled_frame_rejects_without_buffering_body() {
    // Arrange
    let duration = Duration::from_millis(20);
    let mut oversized = Vec::new();
    prost::encoding::encode_varint((FrameKind::Control.max_frame() + 1) as u64, &mut oversized);
    let cases = [(oversized, true), (vec![1], false)];
    for (prefix, too_large) in cases {
        let prefix_len = prefix.len();
        let mut stream = StalledStream {
            prefix: Some(prefix),
            reads: 0,
        };
        let mut buf = Vec::new();

        // Act
        let result = read::<Envelope>(
            &mut stream,
            &mut buf,
            FrameKind::Control,
            ChunkBound::Fixed(FrameKind::Control.max_frame()),
            Some(duration),
        )
        .await;

        // Assert
        if too_large {
            assert!(matches!(
                result,
                Err(LinkError::Proto(ProtoError::FrameTooLarge { .. }))
            ));
            assert_eq!(stream.reads, 1);
        } else {
            assert!(matches!(result, Err(LinkError::Timeout)));
        }
        assert_eq!(buf.len(), prefix_len);
        assert!(buf.capacity() < FrameKind::Control.max_frame());
    }
}

#[tokio::test(start_paused = true)]
async fn read_waiting_for_first_byte_keeps_caller_deadline() {
    // Arrange
    let duration = Duration::from_millis(20);
    let caller_deadline = duration * 2;
    let mut stream = StalledStream {
        prefix: None,
        reads: 0,
    };
    let mut buf = Vec::new();

    // Act
    let result = timeout(
        caller_deadline,
        read::<Envelope>(
            &mut stream,
            &mut buf,
            FrameKind::Control,
            ChunkBound::Fixed(usize::MAX),
            Some(duration),
        ),
    )
    .await;

    // Assert
    assert!(result.is_err());
    assert_eq!(buf, [] as [u8; 0]);
}
