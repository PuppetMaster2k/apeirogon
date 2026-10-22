// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_proto::limits;
use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

use crate::{IpcError, MAX_LINE_LENGTH};

/// Taking the stream out before I/O makes errors and cancellation close it.
#[derive(Debug)]
pub(crate) struct JsonLines<S> {
    stream: Option<BufReader<S>>,
}

impl<S: AsyncRead + AsyncWrite + Unpin> JsonLines<S> {
    pub(crate) fn new(stream: S) -> Self {
        Self {
            stream: Some(BufReader::new(stream)),
        }
    }

    pub(crate) async fn read<T: DeserializeOwned>(&mut self) -> Result<T, IpcError> {
        let mut stream = self.stream.take().ok_or(IpcError::Closed)?;
        let value = tokio::time::timeout(limits::FRAME_READ_TIMEOUT, read_line(&mut stream))
            .await
            .map_err(|_| IpcError::Timeout)??;
        self.stream = Some(stream);
        Ok(value)
    }

    pub(crate) async fn write<T: Serialize>(&mut self, value: &T) -> Result<(), IpcError> {
        let mut stream = self.stream.take().ok_or(IpcError::Closed)?;
        let mut bytes = serde_json::to_vec(value)?;
        if bytes.len() > MAX_LINE_LENGTH {
            return Err(IpcError::LineTooLong);
        }
        bytes.push(b'\n');
        tokio::time::timeout(limits::FRAME_READ_TIMEOUT, async {
            stream.get_mut().write_all(&bytes).await?;
            stream.get_mut().flush().await
        })
        .await
        .map_err(|_| IpcError::Timeout)??;
        self.stream = Some(stream);
        Ok(())
    }
}

async fn read_line<S: AsyncRead + Unpin, T: DeserializeOwned>(
    stream: &mut BufReader<S>,
) -> Result<T, IpcError> {
    let mut line = Vec::new();
    loop {
        let chunk = stream.fill_buf().await?;
        if chunk.is_empty() {
            return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof).into());
        }
        let end = chunk.iter().position(|&byte| byte == b'\n');
        let count = end.unwrap_or(chunk.len());
        if count > MAX_LINE_LENGTH - line.len() {
            return Err(IpcError::LineTooLong);
        }
        line.extend_from_slice(&chunk[..count]);
        stream.consume(count + usize::from(end.is_some()));
        if end.is_some() {
            return Ok(serde_json::from_slice(&line)?);
        }
    }
}

#[cfg(test)]
mod tests;
