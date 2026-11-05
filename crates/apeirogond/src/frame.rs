// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared length-prefixed stream reader; callers retain their deadline policies.

use std::future::Future;
use std::time::Duration;

use a7n_proto::{FrameKind, decode_frame};
use prost::Message;
use quinn::{Chunk, ReadError, RecvStream};
use tokio::time::timeout;

use crate::link::LinkError;

/// A test seam for stalled and fragmented reads without real sockets or sleeps.
pub(crate) trait FrameStream: Send {
    fn read_chunk(
        &mut self,
        max: usize,
    ) -> impl Future<Output = Result<Option<Chunk>, ReadError>> + Send;
}

impl FrameStream for RecvStream {
    async fn read_chunk(&mut self, max: usize) -> Result<Option<Chunk>, ReadError> {
        self.read_chunk(max, true).await
    }
}

pub(crate) enum ChunkBound {
    Fixed(usize),
    RemainingFrame,
}

/// Keeps surplus bytes and partial frames in `buf`, including on cancellation.
/// `chunk_timeout` starts only after the first byte, as required by control reads.
pub(crate) async fn read<M: Message + Default>(
    stream: &mut impl FrameStream,
    buf: &mut Vec<u8>,
    kind: FrameKind,
    bound: ChunkBound,
    chunk_timeout: Option<Duration>,
) -> Result<Option<M>, LinkError> {
    loop {
        if let Some((message, used)) = decode_frame::<M>(buf, kind)? {
            buf.drain(..used);
            return Ok(Some(message));
        }
        let max = match bound {
            ChunkBound::Fixed(max) => max,
            ChunkBound::RemainingFrame => kind.max_frame().saturating_sub(buf.len()).max(1),
        };
        let read = stream.read_chunk(max);
        let chunk = match chunk_timeout.filter(|_| !buf.is_empty()) {
            Some(duration) => timeout(duration, read)
                .await
                .map_err(|_| LinkError::Timeout)??,
            None => read.await?,
        };
        match chunk {
            Some(chunk) => buf.extend_from_slice(&chunk.bytes),
            None if buf.is_empty() => return Ok(None),
            None => return Err(LinkError::StreamClosed),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
