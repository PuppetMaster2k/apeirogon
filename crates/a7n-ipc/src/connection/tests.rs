// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

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
