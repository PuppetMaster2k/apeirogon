// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_proto::MachineId;

use super::*;
use crate::{NearbyMachine, PeerInfo, StatusInfo};

#[tokio::test]
async fn request_status_returns_same_status_info() {
    // Arrange
    let (client_stream, server_stream) = tokio::io::duplex(1024);
    let status = StatusInfo {
        input: None,
        paused: false,
        focused: MachineId::try_from([1; MachineId::LEN].as_slice()).unwrap(),
        pid: std::process::id(),
        machine: MachineId::try_from([1; MachineId::LEN].as_slice()).unwrap(),
        name: "own".to_owned(),
        peers: vec![PeerInfo {
            id: MachineId::try_from([2; MachineId::LEN].as_slice()).unwrap(),
            name: "peer".to_owned(),
            connected: true,
            rtt_ms: Some(7),
        }],
        nearby: vec![NearbyMachine {
            id: MachineId::try_from([3; MachineId::LEN].as_slice()).unwrap(),
            addr: "127.0.0.1:42042".parse().unwrap(),
        }],
    };
    let expected = status.clone();
    let server = async move {
        let mut connection = IpcConnection::accept_stream(server_stream).await.unwrap();
        for _ in 0..2 {
            assert!(matches!(
                connection.read_request().await.unwrap(),
                Request::Status
            ));
            connection
                .write_response(&Response::Status(status.clone()))
                .await
                .unwrap();
        }
    };
    let client = async move {
        let mut client = IpcClient::connect_stream(client_stream).await.unwrap();
        let mut replies = Vec::new();
        for _ in 0..2 {
            let Response::Status(received) = client.request(&Request::Status).await.unwrap() else {
                panic!("expected status")
            };
            replies.push(received);
        }
        replies
    };

    // Act
    let ((), replies) = tokio::join!(server, client);

    // Assert
    assert_eq!(replies, vec![expected.clone(), expected]);
}

#[tokio::test]
async fn accept_stream_version_mismatch_replies_with_error_then_closes() {
    // Arrange
    let (client_stream, server_stream) = tokio::io::duplex(1024);
    let client = async {
        let mut lines = JsonLines::new(client_stream);
        lines
            .write(&Hello {
                ipc_version: IPC_VERSION + 1,
            })
            .await
            .unwrap();
        let reply: Response = lines.read().await.unwrap();
        let eof = lines.read::<Response>().await;
        (reply, eof)
    };

    // Act
    let (server_result, (response, eof)) =
        tokio::join!(IpcConnection::accept_stream(server_stream), client);

    // Assert
    assert!(matches!(
        server_result,
        Err(IpcError::UnsupportedVersion { .. })
    ));
    assert!(matches!(
        response,
        Response::Error {
            kind: ErrorKind::UnsupportedVersion,
            ..
        }
    ));
    assert!(
        matches!(eof, Err(IpcError::Io(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof)
    );
}

#[tokio::test]
async fn connect_stream_server_version_mismatch_is_refused() {
    // Arrange
    let (client_stream, server_stream) = tokio::io::duplex(1024);
    let server = async {
        let mut lines = JsonLines::new(server_stream);
        let _: Hello = lines.read().await.unwrap();
        lines
            .write(&Hello {
                ipc_version: IPC_VERSION + 1,
            })
            .await
            .unwrap();
    };

    // Act
    let (result, ()) = tokio::join!(IpcClient::connect_stream(client_stream), server);

    // Assert
    assert!(matches!(result, Err(IpcError::UnsupportedVersion { .. })));
}

#[tokio::test(start_paused = true)]
async fn accept_stream_silent_client_times_out_and_closes() {
    // Arrange
    let (mut client, server) = tokio::io::duplex(1024);

    // Act
    let result = IpcConnection::accept_stream(server).await;

    // Assert
    assert!(matches!(result, Err(IpcError::Timeout)));
    assert_eq!(
        tokio::io::AsyncReadExt::read_u8(&mut client)
            .await
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::UnexpectedEof
    );
}

#[tokio::test]
async fn connect_stream_error_response_returns_typed_refusal() {
    // Arrange
    let (client, server) = tokio::io::duplex(1024);
    let server = async {
        let mut lines = JsonLines::new(server);
        let _: Hello = lines.read().await.unwrap();
        lines
            .write(&Response::Error {
                kind: ErrorKind::UnsupportedVersion,
                message: "update the client".to_owned(),
            })
            .await
            .unwrap();
    };

    // Act
    let (result, ()) = tokio::join!(IpcClient::connect_stream(client), server);

    // Assert
    assert!(matches!(
        result,
        Err(IpcError::HandshakeRefused {
            kind: ErrorKind::UnsupportedVersion,
            ..
        })
    ));
}
