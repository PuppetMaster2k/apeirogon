// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

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
