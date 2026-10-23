// SPDX-License-Identifier: GPL-3.0-or-later

//! File helpers for secrets and trust data.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Per-user daemon log path.
///
/// # Errors
/// Returns an error if the user data directory cannot be found.
pub fn log_path() -> io::Result<PathBuf> {
    directories::ProjectDirs::from("", "", "apeirogon")
        .map(|dirs| dirs.data_local_dir().join("apeirogond.log"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home directory found"))
}

/// Opens an append-only log with user-only permissions.
///
/// # Errors
/// Returns any directory, permission or file I/O error.
pub fn append_user_only(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(USER_ONLY_MODE);
        let file = options.open(path)?;
        file.set_permissions(fs::Permissions::from_mode(USER_ONLY_MODE))?;
        Ok(file)
    }
    #[cfg(not(unix))]
    options.open(path)
}

/// Permission bits of a user-only file on unix.
#[cfg(unix)]
const USER_ONLY_MODE: u32 = 0o600;

/// Writes `bytes` to `path` atomically: a temp file in the same directory is
/// created user-only, synced, then renamed over `path`.
///
/// # Errors
/// Returns any I/O error from creating, writing or renaming the file.
pub fn write_user_only(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut temp_name = path.file_name().unwrap_or_default().to_os_string();
    temp_name.push(".tmp");
    let temp_path = path.with_file_name(temp_name);

    let result = write_temp(&temp_path, bytes).and_then(|()| fs::rename(&temp_path, path));
    if result.is_err() {
        // Best effort: the original error is the one worth reporting.
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn write_temp(temp_path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = create_user_only(temp_path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(unix)]
fn create_user_only(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(USER_ONLY_MODE)
        .open(path)
}

// ponytail: no explicit ACL; the per-user data directory is already user-only
// by its inherited ACL. Tighten if the key ever moves outside the profile.
#[cfg(not(unix))]
fn create_user_only(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn write_user_only_new_file_is_user_only() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret");

        // Act
        write_user_only(&path, b"key").unwrap();

        // Assert
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, USER_ONLY_MODE);
    }
}

/// Compares open file identities, including across rename and rotation.
///
/// # Errors
/// Returns any metadata query error.
#[cfg(unix)]
pub fn same_file(left: &File, right: &File) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let (left, right) = (left.metadata()?, right.metadata()?);
    Ok(left.dev() == right.dev() && left.ino() == right.ino())
}

/// Compares open file identities, including across rename and rotation.
///
/// # Errors
/// Returns any metadata query error.
#[cfg(windows)]
pub fn same_file(left: &File, right: &File) -> io::Result<bool> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    fn identity(file: &File) -> io::Result<(u32, u32, u32)> {
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: file owns a live handle and info is writable for the call.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &raw mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((
            info.dwVolumeSerialNumber,
            info.nFileIndexHigh,
            info.nFileIndexLow,
        ))
    }
    Ok(identity(left)? == identity(right)?)
}
