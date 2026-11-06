// SPDX-License-Identifier: GPL-3.0-or-later

//! Filesystem operations used at the file-transfer trust boundary.

#[cfg(windows)]
use std::fs;
use std::fs::File;
use std::io;
use std::path::Path;

fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

/// Free bytes available to this user in the directory's filesystem.
/// Missing directories are resolved through their nearest existing ancestor.
///
/// # Errors
/// Returns filesystem query errors or an overflow in the reported byte count.
pub fn available_space(path: &Path) -> io::Result<u64> {
    let absolute = std::path::absolute(path)?;
    let mut path = absolute.as_path();
    loop {
        match std::fs::metadata(path) {
            Ok(_) => return directory_available_space(path),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                path = path.parent().ok_or(error)?;
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(unix)]
fn directory_available_space(path: &Path) -> io::Result<u64> {
    let stats = rustix::fs::statvfs(path)?;
    stats
        .f_bavail
        .checked_mul(stats.f_frsize)
        .ok_or_else(|| io::Error::other("available space overflow"))
}

#[cfg(windows)]
fn directory_available_space(path: &Path) -> io::Result<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    // UNC share roots require a trailing separator.
    let path = path.join("");
    let path: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    if path[..path.len() - 1].contains(&0) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut available = 0;
    // SAFETY: the path is NUL-terminated; the output is writable and other outputs are optional.
    if unsafe {
        GetDiskFreeSpaceExW(
            path.as_ptr(),
            &raw mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(available)
}

/// Create missing directories without following symlinks, including in ancestors.
///
/// # Errors
/// Rejects symlinks, non-directories and filesystem failures.
#[cfg(unix)]
pub fn create_directory(path: &Path) -> io::Result<()> {
    directory(path, true).map(|_| ())
}

#[cfg(unix)]
fn directory(path: &Path, create: bool) -> io::Result<File> {
    use rustix::fs::{CWD, Mode, OFlags, mkdirat, openat};
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut directory = openat(CWD, "/", flags, Mode::empty())?;
    for part in std::path::absolute(path)?.components() {
        if matches!(part, std::path::Component::RootDir) {
            continue;
        }
        if create {
            match mkdirat(&directory, part.as_os_str(), Mode::RWXU) {
                Ok(()) | Err(rustix::io::Errno::EXIST) => {}
                Err(error) => return Err(error.into()),
            }
        }
        directory = openat(&directory, part.as_os_str(), flags, Mode::empty())?;
    }
    Ok(File::from(directory))
}

/// Check every ancestor and create missing directories individually.
///
/// # Errors
/// Rejects reparse points, non-directories and filesystem failures.
#[cfg(windows)]
pub fn create_directory(path: &Path) -> io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    let mut parent = std::path::PathBuf::new();
    let mut ancestors = Vec::new();
    for component in path.components() {
        parent.push(component);
        // A bare drive prefix (`C:`, `\\?\C:`) names the volume, not a directory.
        if matches!(component, std::path::Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&parent) {
            Ok(metadata)
                if metadata.is_dir()
                    && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0 => {}
            Ok(_) => return Err(io::Error::other("unsafe directory")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir(&parent)?,
            Err(error) => return Err(error),
        }
        ancestors.push(windows_directory(&parent)?);
    }
    Ok(())
}

#[cfg(windows)]
fn windows_directory(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let dir = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    let meta = dir.metadata()?;
    if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::other("unsafe directory"));
    }
    Ok(dir)
}

#[cfg(windows)]
fn windows_ancestors(path: &Path) -> io::Result<Vec<File>> {
    let mut parent = Some(path);
    let mut ancestors = Vec::new();
    while let Some(path) = parent {
        if path.as_os_str().is_empty() {
            break;
        }
        ancestors.push(windows_directory(path)?);
        parent = path.parent();
    }
    Ok(ancestors)
}

/// Check that an existing directory and its ancestors contain no symlinks.
///
/// # Errors
/// Rejects missing directories, symlinks and filesystem failures.
pub fn check_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        directory(path, false).map(|_| ())
    }
    #[cfg(windows)]
    {
        let _ancestors = windows_ancestors(path)?;
        Ok(())
    }
}

/// How [`open_file`] opens a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    Read,
    Write,
    /// Read-write with exclusive creation; fails if the file exists.
    CreateNew,
}

/// Open a regular file without following symlinks.
///
/// # Errors
/// Rejects unsafe paths, non-regular files and I/O failures.
#[cfg(unix)]
pub fn open_file(path: &Path, mode: OpenMode) -> io::Result<File> {
    use rustix::fs::{Mode, OFlags, openat};
    let parent = directory(parent(path), false)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("missing file name"))?;
    let flags = match mode {
        OpenMode::Read => OFlags::RDONLY,
        OpenMode::Write => OFlags::RDWR,
        OpenMode::CreateNew => OFlags::RDWR | OFlags::CREATE | OFlags::EXCL,
    };
    let file = File::from(openat(
        &parent,
        name,
        flags | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )?);
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("not a regular file"));
    }
    Ok(file)
}

/// Open a regular file without following reparse points.
///
/// # Errors
/// Rejects symlinks, non-regular files and I/O failures.
#[cfg(windows)]
pub fn open_file(path: &Path, mode: OpenMode) -> io::Result<File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
    };
    // Holding ancestors without delete sharing prevents replacement during the open.
    let _ancestors = windows_ancestors(parent(path))?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(mode != OpenMode::Read)
        .create_new(mode == OpenMode::CreateNew)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::other("not a regular file"));
    }
    Ok(file)
}

/// Publish an entry atomically without replacing an existing destination.
///
/// # Errors
/// Returns `AlreadyExists` on collision, or a filesystem error.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use rustix::fs::{RenameFlags, renameat_with};
    let source = directory(parent(from), false)?;
    let destination = directory(parent(to), false)?;
    renameat_with(
        &source,
        from.file_name()
            .ok_or_else(|| io::Error::other("missing name"))?,
        &destination,
        to.file_name()
            .ok_or_else(|| io::Error::other("missing name"))?,
        RenameFlags::NOREPLACE,
    )
    .map_err(Into::into)
}

/// Publish an entry without replacing an existing destination.
///
/// # Errors
/// Returns the filesystem error, including name collisions.
#[cfg(windows)]
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    let _source = windows_ancestors(parent(from))?;
    let _destination = windows_ancestors(parent(to))?;
    let destination = to;
    let from: Vec<_> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<_> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: both paths are NUL-terminated and valid for the duration of the call.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) } == 0 {
        let error = io::Error::last_os_error();
        if fs::symlink_metadata(destination).is_ok() {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn available_space_missing_directory_queries_existing_ancestor() {
        // Arrange
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing/nested");

        // Act
        let result = available_space(&missing);

        // Assert
        assert!(result.is_ok(), "{result:?}");
        assert!(!missing.exists());
    }

    #[test]
    fn rename_no_replace_existing_file_or_directory_preserves_both_entries() {
        for directories in [false, true] {
            // Arrange
            let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
            let from = temp.path().join("from");
            let to = temp.path().join("to");
            if directories {
                fs::create_dir(&from).unwrap();
                fs::create_dir(&to).unwrap();
            } else {
                fs::write(&from, b"new").unwrap();
                fs::write(&to, b"existing").unwrap();
            }

            // Act
            let result = rename_no_replace(&from, &to);

            // Assert
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
            assert!(from.exists());
            if !directories {
                assert_eq!(fs::read(to).unwrap(), b"existing");
            }
        }
    }

    #[test]
    fn create_directory_canonical_path_creates_nested_directories() {
        // Arrange
        // On Windows `canonicalize` returns a verbatim `\\?\C:\...` path.
        let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let nested = temp.path().join("stage").join("nested");

        // Act
        let result = create_directory(&nested);

        // Assert
        assert!(result.is_ok(), "{result:?}");
        assert!(nested.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn open_file_symlink_ancestor_or_leaf_is_rejected() {
        // Arrange
        let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let actual = temp.path().join("actual");
        fs::create_dir(&actual).unwrap();
        fs::write(actual.join("file"), b"private").unwrap();
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&actual, &link).unwrap();
        std::os::unix::fs::symlink(actual.join("file"), temp.path().join("leaf")).unwrap();

        // Act
        let through_parent = open_file(&link.join("file"), OpenMode::Read);
        let through_leaf = open_file(&temp.path().join("leaf"), OpenMode::Read);
        let create_below_link = create_directory(&link.join("child"));

        // Assert
        assert!(through_parent.is_err());
        assert!(through_leaf.is_err());
        assert!(create_below_link.is_err());
        assert!(!actual.join("child").exists());
    }
}
