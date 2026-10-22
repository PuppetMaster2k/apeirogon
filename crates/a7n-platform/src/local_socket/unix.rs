// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    fs, io,
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, FileTypeExt, MetadataExt},
    },
    path::Path,
};

use a7n_proto::limits;
use interprocess::{
    local_socket::{GenericFilePath, ListenerOptions, tokio::prelude::*},
    os::unix::local_socket::ListenerOptionsExt,
};
use rustix::{
    fs::{Mode, RawMode},
    process::geteuid,
};

use super::{Listener, SocketError, SocketLocation, Stream};

const DIRECTORY_MODE: u32 = 0o700;
const SOCKET_MODE: RawMode = 0o600;
const PERMISSION_BITS: u32 = 0o7777;
// Leave room for the terminating NUL on macOS as well as Linux.
const MAX_SOCKET_PATH_BYTES: usize = 103;

pub(super) fn default_location() -> Result<SocketLocation, SocketError> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|dirs| dirs.cache_dir().to_owned()))
        .ok_or(SocketError::NoUserLocation)?;
    let location = SocketLocation::new(base.join("apeirogon/ipc.sock"));
    validate_path(location.as_path())?;
    Ok(location)
}

fn validate_path(path: &Path) -> Result<(), SocketError> {
    if !path.is_absolute() || path.as_os_str().as_bytes().len() > MAX_SOCKET_PATH_BYTES {
        return Err(SocketError::InvalidPath {
            path: path.to_owned(),
            max_bytes: MAX_SOCKET_PATH_BYTES,
        });
    }
    Ok(())
}

fn private_directory(path: &Path) -> Result<(), SocketError> {
    match fs::DirBuilder::new()
        .recursive(true)
        .mode(DIRECTORY_MODE)
        .create(path)
    {
        Ok(()) => (),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
        Err(error) => return Err(error.into()),
    }
    check_directory(path)
}

fn check_directory(path: &Path) -> Result<(), SocketError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.uid() != geteuid().as_raw()
        || metadata.mode() & PERMISSION_BITS != DIRECTORY_MODE
    {
        return Err(SocketError::InsecureDirectory(path.to_owned()));
    }
    Ok(())
}

fn checked_socket(path: &Path) -> Result<Option<fs::Metadata>, SocketError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.file_type().is_socket() || metadata.uid() != geteuid().as_raw() {
        return Err(SocketError::InsecureSocket(path.to_owned()));
    }
    Ok(Some(metadata))
}

async fn remove_stale_socket(location: &SocketLocation) -> Result<(), SocketError> {
    let path = location.as_path();
    let Some(before) = checked_socket(path)? else {
        return Ok(());
    };
    match connect(location).await {
        Ok(_) => return Err(io::Error::from(io::ErrorKind::AddrInUse).into()),
        Err(SocketError::Io(error)) if error.kind() == io::ErrorKind::ConnectionRefused => (),
        Err(error) => return Err(error),
    }
    if let Some(after) = checked_socket(path)? {
        if before.dev() != after.dev() || before.ino() != after.ino() {
            return Err(io::Error::from(io::ErrorKind::AddrInUse).into());
        }
        fs::remove_file(path)?;
    }
    Ok(())
}

pub(super) async fn bind_user_only(location: &SocketLocation) -> Result<Listener, SocketError> {
    let path = location.as_path();
    validate_path(path)?;
    let parent = path.parent().ok_or_else(|| SocketError::InvalidPath {
        path: path.to_owned(),
        max_bytes: MAX_SOCKET_PATH_BYTES,
    })?;
    private_directory(parent)?;
    remove_stale_socket(location).await?;
    let name = path.to_fs_name::<GenericFilePath>()?;
    let listener = match ListenerOptions::new()
        .name(name.clone())
        .mode(SOCKET_MODE)
        .create_tokio()
    {
        Ok(listener) => listener,
        Err(error) if error.kind() == io::ErrorKind::Unsupported => {
            // The verified private directory protects the socket until chmod on macOS.
            ListenerOptions::new().name(name).create_tokio()?
        }
        Err(error) => return Err(error.into()),
    };
    rustix::fs::chmod(path, Mode::from_raw_mode(SOCKET_MODE)).map_err(io::Error::from)?;
    Ok(listener)
}

pub(super) async fn connect(location: &SocketLocation) -> Result<Stream, SocketError> {
    let path = location.as_path();
    validate_path(path)?;
    let parent = path.parent().ok_or_else(|| SocketError::InvalidPath {
        path: path.to_owned(),
        max_bytes: MAX_SOCKET_PATH_BYTES,
    })?;
    // Clients may send a pairing secret, so they must also reject an impostor's endpoint.
    check_directory(parent)?;
    checked_socket(path)?;
    let name = path.to_fs_name::<GenericFilePath>()?;
    tokio::time::timeout(limits::HANDSHAKE_TIMEOUT, Stream::connect(name))
        .await
        .map_err(|_| SocketError::Timeout)?
        .map_err(Into::into)
}

#[cfg(test)]
#[path = "unix/tests.rs"]
mod tests;
