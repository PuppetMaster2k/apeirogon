// SPDX-License-Identifier: GPL-3.0-or-later

use super::super::receive::check_space;
use super::*;

#[test]
fn check_space_insufficient_bytes_or_margin_refuses_receive() {
    // Arrange
    let directory = std::path::Path::new("receive");

    // Act
    let result = check_space(directory, 1, 0, |path| {
        assert_eq!(path, directory);
        Ok(256 * 1024 * 1024)
    });

    // Assert
    assert_eq!(result, Err(Failure::NoSpace));
}

#[test]
fn check_space_resumed_bytes_requires_only_remainder_and_margin() {
    // Arrange
    let directory = std::path::Path::new("receive");
    let total = 1024 * 1024 * 1024;
    let remainder = 7;

    // Act
    let result = check_space(directory, total, total - remainder, |_| {
        Ok(256 * 1024 * 1024 + remainder)
    });

    // Assert
    assert_eq!(result, Ok(()));
}

#[test]
fn check_space_lookup_failure_allows_receive() {
    // Arrange
    let directory = std::path::Path::new("receive");

    // Act
    let result = check_space(directory, 1, 0, |_| Err(std::io::ErrorKind::Other.into()));

    // Assert
    assert_eq!(result, Ok(()));
}

#[test]
fn check_space_required_bytes_overflow_refuses_receive() {
    // Arrange
    let directory = std::path::Path::new("receive");

    // Act
    let result = check_space(directory, u64::MAX, 0, |_| Ok(u64::MAX));

    // Assert
    assert_eq!(result, Err(Failure::NoSpace));
}

#[tokio::test]
async fn files_finished_no_space_reports_distinct_accept_failure() {
    // Arrange
    let (mut daemon, _dir) = daemon();
    let peer = MachineId::try_from([1; MachineId::LEN].as_slice()).unwrap();
    let (reply, response) = oneshot::channel();
    daemon.files.accepts.insert((peer, 1), reply);
    let (cancel, _) = watch::channel(Failure::Cancelled);
    daemon.files.active.insert(
        1,
        Active {
            peer,
            link: 0,
            wire_id: 1,
            offer: 1,
            kind: Kind::Offer,
            direction: TransferDirection::Receive,
            cancel,
            progress: Arc::new(Progress::default()),
        },
    );

    // Act
    daemon.files_finished(1, Err(Failure::NoSpace));

    // Assert
    let error = response.await.unwrap().unwrap_err();
    assert!(matches!(error, DaemonError::NoSpace));
    assert_eq!(
        error.to_string(),
        "not enough free space in the receive directory"
    );
    assert!(daemon.files.active.is_empty());
    assert!(daemon.files.accepts.is_empty());
}
