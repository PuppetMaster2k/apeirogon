// SPDX-License-Identifier: GPL-3.0-or-later

use std::os::unix::{
    fs::{PermissionsExt, symlink},
    net::UnixListener,
};

use super::*;

#[test]
fn default_location_user_environment_uses_runtime_or_cache_directory() {
    // Arrange
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .map_or_else(
            || directories::BaseDirs::new().unwrap().cache_dir().to_owned(),
            std::path::PathBuf::from,
        );

    // Act
    let location = default_location().unwrap();

    // Assert
    assert_eq!(location.as_path(), base.join("apeirogon/ipc.sock"));
}

#[tokio::test]
async fn bind_user_only_wide_directory_is_refused() {
    // Arrange
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let location = SocketLocation::new(directory.path().join("ipc.sock"));

    // Act
    let result = bind_user_only(&location).await;

    // Assert
    assert!(matches!(result, Err(SocketError::InsecureDirectory(_))));
    assert!(!location.as_path().exists());
}

#[tokio::test]
async fn bind_user_only_private_directory_sets_socket_permissions() {
    // Arrange
    let directory = tempfile::tempdir().unwrap();
    let location = SocketLocation::new(directory.path().join("private/ipc.sock"));

    // Act
    let listener = bind_user_only(&location).await.unwrap();

    // Assert
    assert_eq!(
        fs::symlink_metadata(location.as_path().parent().unwrap())
            .unwrap()
            .mode()
            & PERMISSION_BITS,
        DIRECTORY_MODE
    );
    assert_eq!(
        Mode::from_raw_mode(rustix::fs::stat(location.as_path()).unwrap().st_mode).as_raw_mode(),
        SOCKET_MODE
    );
    drop(listener);
}

#[tokio::test]
async fn bind_user_only_symlink_directory_is_refused() {
    // Arrange
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target");
    fs::DirBuilder::new()
        .mode(DIRECTORY_MODE)
        .create(&target)
        .unwrap();
    let link = directory.path().join("link");
    symlink(target, &link).unwrap();
    let location = SocketLocation::new(link.join("ipc.sock"));

    // Act
    let result = bind_user_only(&location).await;

    // Assert
    assert!(matches!(result, Err(SocketError::InsecureDirectory(_))));
}

#[tokio::test]
async fn bind_user_only_existing_non_socket_is_preserved() {
    // Arrange
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(DIRECTORY_MODE)).unwrap();
    let location = SocketLocation::new(directory.path().join("ipc.sock"));
    fs::write(location.as_path(), b"keep").unwrap();

    // Act
    let result = bind_user_only(&location).await;

    // Assert
    assert!(matches!(result, Err(SocketError::InsecureSocket(_))));
    assert_eq!(fs::read(location.as_path()).unwrap(), b"keep");
}

#[tokio::test]
async fn bind_user_only_stale_socket_is_replaced() {
    // Arrange
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(DIRECTORY_MODE)).unwrap();
    let location = SocketLocation::new(directory.path().join("ipc.sock"));
    drop(UnixListener::bind(location.as_path()).unwrap());

    // Act
    let listener = bind_user_only(&location).await;

    // Assert
    assert!(listener.is_ok());
}

#[tokio::test]
async fn bind_user_only_live_socket_is_preserved() {
    // Arrange
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(DIRECTORY_MODE)).unwrap();
    let location = SocketLocation::new(directory.path().join("ipc.sock"));
    let listener = bind_user_only(&location).await.unwrap();

    // Act
    let result = bind_user_only(&location).await;

    // Assert
    assert!(
        matches!(result, Err(SocketError::Io(error)) if error.kind() == io::ErrorKind::AddrInUse)
    );
    assert!(connect(&location).await.is_ok());
    drop(listener);
}

#[tokio::test]
async fn connect_insecure_directory_is_refused_before_sending_secrets() {
    // Arrange
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let location = SocketLocation::new(directory.path().join("ipc.sock"));
    let _listener = UnixListener::bind(location.as_path()).unwrap();

    // Act
    let result = connect(&location).await;

    // Assert
    assert!(matches!(result, Err(SocketError::InsecureDirectory(_))));
}

#[test]
fn validate_path_relative_or_long_returns_clear_error() {
    // Arrange
    let paths = [
        std::path::PathBuf::from("relative.sock"),
        std::path::PathBuf::from(format!("/{}", "a".repeat(MAX_SOCKET_PATH_BYTES))),
    ];

    for path in paths {
        // Act
        let result = validate_path(&path);

        // Assert
        assert!(matches!(result, Err(SocketError::InvalidPath { .. })));
    }
}
