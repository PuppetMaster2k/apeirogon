// SPDX-License-Identifier: GPL-3.0-or-later

//! Framing and stream operations shared by clipboard and file transfers.

use std::future::Future;
use std::time::Duration;

use a7n_proto::v1::{
    AbortReason, TransferAbort, TransferFrame, TransferOpen, transfer_frame::Frame,
};
use a7n_proto::{FrameKind, encode_frame, limits};
use quinn::{Connection, SendStream};
use tokio::time::timeout;

use crate::frame::ChunkBound;
use crate::link::LinkError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{}", self.reason())]
pub(in crate::daemon) enum TransferError {
    Frame,
    Read,
    Truncated,
    Write,
    Open,
    Priority,
    Finish,
    Stopped,
    Timeout,
    LinkLost,
    TrailingFrame,
    Size,
    Chunk,
    Integrity,
    Image,
    NotFound,
    Refused,
    Abort,
    Ordering,
    Unavailable,
    Busy,
    TransferIdExhausted,
    Stale,
    OfferWorker,
    SourceName,
    EmptyOffer,
    Manifest,
    ManifestTooLarge,
    SourceMetadata,
    TooManyEntries,
    SourceOpen,
    SourceHash,
    SourceChanged,
    SourceKind,
    SizesOverflow,
    SourceDirectoryUnsafe,
    SourceDirectory,
}

impl TransferError {
    pub(in crate::daemon) fn reason(self) -> &'static str {
        match self {
            Self::Frame => "frame",
            Self::Read => "read",
            Self::Truncated => "truncated",
            Self::Write => "write",
            Self::Open => "open",
            Self::Priority => "priority",
            Self::Finish => "finish",
            Self::Stopped => "stopped",
            Self::Timeout => "timeout",
            Self::LinkLost => "link_lost",
            Self::TrailingFrame => "trailing_frame",
            Self::Size => "size",
            Self::Chunk => "chunk",
            Self::Integrity => "integrity",
            Self::Image => "image",
            Self::NotFound => "not_found",
            Self::Refused => "refused",
            Self::Abort => "abort",
            Self::Ordering => "ordering",
            Self::Unavailable => "unavailable",
            Self::Busy => "busy",
            Self::TransferIdExhausted => "transfer_id_exhausted",
            Self::Stale => "stale",
            Self::OfferWorker => "file offer worker failed",
            Self::SourceName => "invalid source name",
            Self::EmptyOffer => "no files to offer",
            Self::Manifest => "invalid file manifest",
            Self::ManifestTooLarge => "file manifest exceeds control frame limit",
            Self::SourceMetadata => "source metadata failed",
            Self::TooManyEntries => "too many file entries",
            Self::SourceOpen => "source open failed",
            Self::SourceHash => "source hash failed",
            Self::SourceChanged => "source changed while hashing",
            Self::SourceKind => "source is not a regular file or directory",
            Self::SizesOverflow => "file sizes overflow",
            Self::SourceDirectoryUnsafe => "unsafe source directory",
            Self::SourceDirectory => "source directory failed",
        }
    }
}

async fn deadline<T>(
    duration: Option<Duration>,
    operation: impl Future<Output = Result<T, TransferError>>,
) -> Result<T, TransferError> {
    match duration {
        Some(duration) => timeout(duration, operation)
            .await
            .map_err(|_| TransferError::Timeout)?,
        None => operation.await,
    }
}

pub(in crate::daemon) async fn write(
    stream: &mut SendStream,
    buf: &mut Vec<u8>,
    frame: Frame,
) -> Result<(), TransferError> {
    buf.clear();
    encode_frame(&TransferFrame { frame: Some(frame) }, buf);
    stream
        .write_all(buf)
        .await
        .map_err(|_| TransferError::Write)
}

pub(in crate::daemon) async fn open(
    connection: &Connection,
    id: u64,
    duration: Option<Duration>,
) -> Result<(SendStream, Vec<u8>), TransferError> {
    let mut stream = deadline(duration, async {
        connection.open_uni().await.map_err(|_| TransferError::Open)
    })
    .await?;
    stream
        .set_priority(-1)
        .map_err(|_| TransferError::Priority)?;
    let mut buf = Vec::new();
    deadline(
        duration,
        write(
            &mut stream,
            &mut buf,
            Frame::Open(TransferOpen { transfer_id: id }),
        ),
    )
    .await?;
    Ok((stream, buf))
}

pub(in crate::daemon) async fn abort(
    stream: &mut SendStream,
    buf: &mut Vec<u8>,
    reason: AbortReason,
    duration: Option<Duration>,
) -> Result<(), TransferError> {
    deadline(
        duration,
        write(
            stream,
            buf,
            Frame::Abort(TransferAbort {
                reason: reason.into(),
                detail: String::new(),
            }),
        ),
    )
    .await?;
    stream.finish().map_err(|_| TransferError::Finish)
}

pub(in crate::daemon) async fn read(
    stream: &mut impl crate::frame::FrameStream,
    buf: &mut Vec<u8>,
) -> Result<Option<Frame>, TransferError> {
    timeout(limits::FRAME_READ_TIMEOUT, async {
        let frame: Option<TransferFrame> = crate::frame::read(
            stream,
            buf,
            FrameKind::Transfer,
            ChunkBound::RemainingFrame,
            None,
        )
        .await
        .map_err(|error| match error {
            LinkError::Proto(_) => TransferError::Frame,
            LinkError::StreamClosed => TransferError::Truncated,
            _ => TransferError::Read,
        })?;
        frame
            .map(|frame| frame.frame.ok_or(TransferError::Frame))
            .transpose()
    })
    .await
    .map_err(|_| TransferError::Timeout)?
}
