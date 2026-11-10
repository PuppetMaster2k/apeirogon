// SPDX-License-Identifier: GPL-3.0-or-later

use tokio::io::AsyncReadExt;

use super::*;

#[tokio::test]
async fn read_oversized_unterminated_line_closes_connection() {
    // Arrange
    let (mut sender, receiver) = tokio::io::duplex(1024);
    let mut lines = JsonLines::new(receiver);
    let writer = async {
        sender
            .write_all(&vec![b'x'; MAX_LINE_LENGTH + 1])
            .await
            .unwrap();
        sender.read_u8().await
    };

    // Act
    let (result, peer_result) = tokio::join!(lines.read::<serde_json::Value>(), writer);

    // Assert
    assert!(matches!(result, Err(IpcError::LineTooLong)));
    assert!(lines.stream.is_none());
    assert!(matches!(peer_result, Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof));
}

#[tokio::test(start_paused = true)]
async fn read_partial_line_times_out_and_closes() {
    // Arrange
    let (mut sender, receiver) = tokio::io::duplex(1024);
    sender.write_all(b"{").await.unwrap();
    let mut lines = JsonLines::new(receiver);

    // Act
    let result = lines.read::<serde_json::Value>().await;

    // Assert
    assert!(matches!(result, Err(IpcError::Timeout)));
    assert!(lines.stream.is_none());
}

#[tokio::test]
async fn write_oversized_message_closes_without_sending_partial_json() {
    // Arrange
    let (sender, mut receiver) = tokio::io::duplex(1024);
    let mut lines = JsonLines::new(sender);
    let value = "x".repeat(MAX_LINE_LENGTH);

    // Act
    let result = lines.write(&value).await;

    // Assert
    assert!(matches!(result, Err(IpcError::LineTooLong)));
    assert_eq!(
        receiver.read_u8().await.unwrap_err().kind(),
        std::io::ErrorKind::UnexpectedEof
    );
}

#[tokio::test]
async fn read_exact_limit_payload_is_accepted() {
    // Arrange
    let value = "x".repeat(MAX_LINE_LENGTH - 2);
    let (sender, receiver) = tokio::io::duplex(1024);
    let mut writer = JsonLines::new(sender);
    let mut reader = JsonLines::new(receiver);

    // Act
    let (write_result, read_result) = tokio::join!(writer.write(&value), reader.read::<String>());

    // Assert
    write_result.unwrap();
    assert_eq!(read_result.unwrap(), value);
}

#[tokio::test]
async fn read_malformed_or_unterminated_json_closes_connection() {
    // Arrange
    let inputs: &[&[u8]] = &[b"{\n", b"{\"type\":\"status\"}"];
    for input in inputs {
        let (mut sender, receiver) = tokio::io::duplex(1024);
        sender.write_all(input).await.unwrap();
        drop(sender);
        let mut lines = JsonLines::new(receiver);

        // Act
        let result = lines.read::<serde_json::Value>().await;

        // Assert
        assert!(result.is_err());
        assert!(matches!(
            lines.read::<serde_json::Value>().await,
            Err(IpcError::Closed)
        ));
    }
}
