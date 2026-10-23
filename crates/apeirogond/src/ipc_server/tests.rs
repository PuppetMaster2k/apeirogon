// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn error_response_daemon_failures_maps_safe_categories() {
    // Arrange
    let cases = [
        (
            DaemonError::Layout(a7n_core::layout::LayoutError::Overlap),
            ErrorKind::InvalidLayout,
        ),
        (DaemonError::PairingBusy, ErrorKind::Busy),
        (DaemonError::NotPaired, ErrorKind::NotPaired),
        (
            DaemonError::Pairing(crate::pairing::PairingError::Refused),
            ErrorKind::PairingRefused,
        ),
        (DaemonError::TrustPoisoned, ErrorKind::Internal),
        (
            DaemonError::Pins(crate::pin_store::PinStoreError::BadHex),
            ErrorKind::Internal,
        ),
        (
            DaemonError::Tls(crate::tls::TlsError::Rustls(rustls::Error::General(
                "TLS configuration failed".into(),
            ))),
            ErrorKind::Internal,
        ),
    ];

    for (error, expected) in cases {
        // Act
        let response = error_response(&error);

        // Assert
        let Response::Error { kind, message } = response else {
            panic!("expected an error")
        };
        assert_eq!(kind, expected);
        assert_eq!(
            message,
            if expected == ErrorKind::PairingRefused {
                "pairing refused".into()
            } else {
                error.to_string()
            }
        );
    }
}

#[tokio::test]
async fn dispatch_closed_command_channel_returns_internal() {
    // Arrange
    let (commands, receiver) = mpsc::channel(1);
    drop(receiver);

    // Act
    let response = dispatch(Request::Status, &commands).await;

    // Assert
    assert!(matches!(
        response,
        Response::Error {
            kind: ErrorKind::Internal,
            ..
        }
    ));
}

#[tokio::test]
async fn dispatch_switch_unlinked_peer_returns_not_connected() {
    // Arrange
    let peer = crate::daemon::test_peer::Peer::start();
    let machine = a7n_proto::MachineId::try_from(&[0; a7n_proto::MachineId::LEN][..]).unwrap();

    // Act
    let response = dispatch(Request::Switch { machine }, peer.sender.as_ref().unwrap()).await;

    // Assert
    assert!(matches!(
        response,
        Response::Error {
            kind: ErrorKind::NotConnected,
            ..
        }
    ));
    peer.stop().await;
}

#[tokio::test]
async fn serve_shutdown_flushes_done_and_stops_with_clients_alive() {
    // Arrange
    let peer = crate::daemon::test_peer::Peer::start();
    let sender = peer.sender.as_ref().unwrap().clone();
    let dir = tempfile::tempdir().unwrap();
    let location = a7n_ipc::SocketLocation::new(dir.path().join("private/control.sock"));
    let server = a7n_ipc::IpcServer::bind_at(&location).await.unwrap();
    let task = tokio::spawn(serve(server, sender.clone()));
    let mut client = a7n_ipc::IpcClient::connect_at(&location).await.unwrap();
    let _idle_client = a7n_ipc::IpcClient::connect_at(&location).await.unwrap();

    // Act
    let response = client.request(&Request::Shutdown).await.unwrap();

    // Assert
    assert!(matches!(response, Response::Done));
    tokio::time::timeout(limits::HANDSHAKE_TIMEOUT, task)
        .await
        .unwrap()
        .unwrap();
    assert!(sender.is_closed());
    peer.stop().await;
}
