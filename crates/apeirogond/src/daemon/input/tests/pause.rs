// SPDX-License-Identifier: GPL-3.0-or-later

use crate::command::Command;

use super::*;

#[tokio::test]
async fn set_paused_driving_returns_home_and_releases_peer_key() {
    // Arrange
    let (a, b, mut a_input, mut b_input) = pair().await;
    drive(&mut a_input, &mut b_input).await;
    a_input
        .capture
        .send(Captured::Input(key(true)))
        .await
        .unwrap();
    b_input.injected().await;
    assert_eq!(a.status().await.focused, b.machine);

    // Act
    a.command(|reply| Command::SetPaused {
        paused: true,
        reply,
    })
    .await
    .unwrap();

    // Assert
    a_input.grab(false).await;
    assert!(matches!(b_input.injected().await, Inject::Event(event) if event == key(false)));
    b_input.grab(false).await;
    let status = a.status().await;
    assert!(status.paused);
    assert_eq!(status.focused, a.machine);
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn switch_machine_paused_rejects_peer_but_allows_home_and_resume() {
    // Arrange
    let (mut daemon, _dir, _input) = daemon(false);
    let target = MachineId::try_from([42; MachineId::LEN].as_slice()).unwrap();
    daemon.set_paused(true).await;

    // Act
    let paused = daemon.switch_machine(target).await;
    let home = daemon.switch_machine(daemon.identity.machine_id).await;
    daemon.set_paused(false).await;
    let resumed = daemon.switch_machine(target).await;

    // Assert
    assert!(matches!(paused, Err(DaemonError::Paused)));
    assert!(home.is_ok());
    assert!(matches!(resumed, Err(DaemonError::NotConnected)));
    assert!(!daemon.status().paused);
}

#[tokio::test]
async fn set_paused_pending_take_ignores_late_accept() {
    // Arrange
    let (mut daemon, _dir, _input) = daemon(false);
    let target = MachineId::try_from([42; MachineId::LEN].as_slice()).unwrap();
    daemon.input.focus.take(
        Take {
            target,
            landing: None,
            resistance_px: 0,
            reach: vec![target],
        },
        now(),
    );

    // Act
    daemon.set_paused(true).await;
    daemon
        .receive_from(
            target,
            Envelope {
                body: Some(Body::Accept(a7n_proto::v1::Accept { route: 1 })),
            },
        )
        .await
        .unwrap();

    // Assert
    assert_eq!(daemon.input.focus.cursor(), Cursor::Home);
    assert!(daemon.input.focus.next_deadline().is_none());
}

#[tokio::test]
async fn captured_paused_edge_and_hotkey_keep_focus_home() {
    // Arrange
    let (a, b, a_input, _b_input) = configured_pair(|a, _| a.input.paused = true).await;
    let jump_chord = [0x0007_00e0, 0x0007_00e2, 0x0007_00e1, 0x0007_001f];

    // Act
    a_input.cross(true).await;
    for usage in jump_chord {
        a_input
            .capture
            .send(Captured::Input(InputEvent {
                event: Some(input_event::Event::Key(KeyEvent {
                    usage,
                    down: true,
                    character: String::new(),
                })),
            }))
            .await
            .unwrap();
    }
    timeout(TEST_TIMEOUT, async {
        while a_input.capture.capacity() != a_input.capture.max_capacity() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let status = a.status().await;

    // Assert
    assert!(status.paused);
    assert_eq!(status.focused, a.machine);
    a.stop().await;
    b.stop().await;
}
