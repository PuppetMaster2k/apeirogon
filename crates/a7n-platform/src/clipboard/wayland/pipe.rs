// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded, non-blocking transfers over the pipes Wayland and the portal hand out.

use std::io;
use std::os::fd::OwnedFd;

use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
use tokio::io::unix::AsyncFd;

const READ_CHUNK: usize = 16 * 1024;

// tokio 1.54 deprecates `new` for a `register` that is not released yet; an
// `OwnedFd` is the I/O-safe case the deprecation is not about.
#[allow(deprecated, reason = "AsyncFd::register is not available yet")]
pub(super) fn async_fd(fd: OwnedFd) -> io::Result<AsyncFd<OwnedFd>> {
    AsyncFd::new(fd)
}

fn register(fd: OwnedFd) -> io::Result<AsyncFd<OwnedFd>> {
    let flags = fcntl_getfl(&fd)?;
    fcntl_setfl(&fd, flags | OFlags::NONBLOCK)?;
    async_fd(fd)
}

/// Reads until end of file. `Ok(None)` means the data exceeds `limit`; the
/// caller bounds the time.
pub(super) async fn read_limited(fd: OwnedFd, limit: usize) -> io::Result<Option<Vec<u8>>> {
    let fd = register(fd)?;
    let mut data = Vec::new();
    let mut chunk = vec![0u8; READ_CHUNK];
    loop {
        let mut ready = fd.readable().await?;
        let read =
            ready.try_io(|fd| rustix::io::read(fd.get_ref(), &mut chunk[..]).map_err(Into::into));
        let count = match read {
            Ok(Ok(count)) => count,
            Ok(Err(error)) if error.kind() == io::ErrorKind::Interrupted => continue,
            Ok(Err(error)) => return Err(error),
            Err(_would_block) => continue,
        };
        if count == 0 {
            return Ok(Some(data));
        }
        if data.len() + count > limit {
            return Ok(None);
        }
        data.extend_from_slice(&chunk[..count]);
    }
}

/// Writes everything, then closes the pipe so the reader sees the end.
pub(super) async fn write_all(fd: OwnedFd, mut data: &[u8]) -> io::Result<()> {
    let fd = register(fd)?;
    while !data.is_empty() {
        let mut ready = fd.writable().await?;
        let write = ready.try_io(|fd| rustix::io::write(fd.get_ref(), data).map_err(Into::into));
        match write {
            Ok(Ok(count)) => data = &data[count..],
            Ok(Err(error)) if error.kind() == io::ErrorKind::Interrupted => {}
            Ok(Err(error)) => return Err(error),
            Err(_would_block) => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustix::pipe::{PipeFlags, pipe_with};

    #[tokio::test]
    async fn read_limited_data_over_limit_returns_none() {
        // Arrange
        let (reader, writer) = pipe_with(PipeFlags::CLOEXEC).unwrap();
        write_all(writer, &[7u8; 9]).await.unwrap();

        // Act
        let result = read_limited(reader, 8).await.unwrap();

        // Assert
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn read_limited_data_at_limit_returns_all_bytes() {
        // Arrange
        let (reader, writer) = pipe_with(PipeFlags::CLOEXEC).unwrap();
        write_all(writer, b"12345678").await.unwrap();

        // Act
        let result = read_limited(reader, 8).await.unwrap();

        // Assert
        assert_eq!(result.as_deref(), Some(&b"12345678"[..]));
    }

    #[tokio::test]
    async fn write_all_reader_closed_returns_error() {
        // Arrange
        let (reader, writer) = pipe_with(PipeFlags::CLOEXEC).unwrap();
        drop(reader);

        // Act
        let result = write_all(writer, b"data").await;

        // Assert
        assert!(result.is_err());
    }
}
