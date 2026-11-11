// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn visibility(fake: &mut FakeInput) -> Vec<bool> {
    std::iter::from_fn(|| fake.control.try_recv().ok())
        .filter_map(|command| match command {
            CaptureControl::CursorHidden(hidden) => Some(hidden),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn input_actions_ended_then_driven_hides_and_shows() {
    // Arrange
    let (mut daemon, _dir, fake) = daemon(true);
    let mut fake = fake.unwrap();
    let driven = Action::Driven {
        owner: daemon.identity.machine_id,
        landing: None,
        resistance_px: 0,
    };

    // Act
    daemon
        .input_actions(vec![Action::EndedDriven, driven])
        .await;

    // Assert
    assert_eq!(visibility(&mut fake), [true, false]);
}

#[tokio::test]
async fn captured_local_motion_restores_hidden_cursor() {
    // Arrange
    let (mut daemon, _dir, fake) = daemon(true);
    let mut fake = fake.unwrap();
    daemon.input_actions(vec![Action::EndedDriven]).await;
    visibility(&mut fake);

    // Act
    daemon
        .captured(Some(Captured::Pointer {
            x: 50,
            y: 50,
            dx: 1,
            dy: 0,
        }))
        .await;

    // Assert
    assert_eq!(visibility(&mut fake), [false]);
}

#[tokio::test]
async fn inject_motion_hidden_cursor_restores_before_injection() {
    // Arrange
    let (mut daemon, _dir, fake) = daemon(true);
    let mut fake = fake.unwrap();
    daemon.input_actions(vec![Action::EndedDriven]).await;
    visibility(&mut fake);

    // Act
    daemon.input.inject(Inject::Motion { dx: 1, dy: 0 }).await;

    // Assert
    assert_eq!(visibility(&mut fake), [false]);
    assert!(matches!(
        fake.inject.try_recv(),
        Ok(Inject::Motion { dx: 1, dy: 0 })
    ));
}

#[tokio::test]
async fn apply_config_hide_disabled_restores_cursor_and_prevents_hiding() {
    // Arrange
    let (mut daemon, _dir, fake) = daemon(true);
    let mut fake = fake.unwrap();
    daemon.input_actions(vec![Action::EndedDriven]).await;
    visibility(&mut fake);
    let mut config = daemon.config.clone();
    config.input.hide_idle_cursor = false;

    // Act
    daemon.apply_config(config).await;
    daemon.input_actions(vec![Action::EndedDriven]).await;

    // Assert
    assert_eq!(visibility(&mut fake), [false]);
}

#[tokio::test]
async fn shutdown_input_idle_or_driving_restores_visibility() {
    // Arrange
    let reasons = [
        Action::EndedDriven,
        Action::Cursor(Cursor::Driving(
            MachineId::try_from(&[2; MachineId::LEN][..]).unwrap(),
        )),
    ];
    for reason in reasons {
        let (mut daemon, _dir, fake) = daemon(true);
        let mut fake = fake.unwrap();
        daemon.input_actions(vec![reason]).await;
        visibility(&mut fake);

        // Act
        daemon.shutdown_input().await;

        // Assert
        assert_eq!(visibility(&mut fake), [false]);
    }
}

#[tokio::test]
async fn input_actions_driving_motion_then_home_hides_until_home() {
    // Arrange
    let (mut daemon, _dir, fake) = daemon(true);
    let mut fake = fake.unwrap();
    let peer = MachineId::try_from(&[2; MachineId::LEN][..]).unwrap();

    // Act
    daemon
        .input_actions(vec![Action::Cursor(Cursor::Driving(peer))])
        .await;
    daemon
        .captured(Some(Captured::Pointer {
            x: 50,
            y: 50,
            dx: 1,
            dy: 0,
        }))
        .await;
    let driving = visibility(&mut fake);
    daemon
        .input_actions(vec![Action::Cursor(Cursor::Home)])
        .await;

    // Assert
    assert_eq!(driving, [true]);
    assert_eq!(visibility(&mut fake), [false]);
}

#[tokio::test]
async fn input_actions_driving_then_driven_with_landing_restores_visibility() {
    // Arrange
    let (mut daemon, _dir, fake) = daemon(true);
    let mut fake = fake.unwrap();
    daemon
        .input_actions(vec![Action::Cursor(Cursor::Driving(
            MachineId::try_from(&[2; MachineId::LEN][..]).unwrap(),
        ))])
        .await;
    visibility(&mut fake);

    // Act
    daemon
        .input_actions(vec![Action::Driven {
            owner: daemon.identity.machine_id,
            landing: Some(a7n_proto::v1::Landing {
                edge: a7n_proto::v1::Edge::Left.into(),
                offset: 0.5,
                ..Default::default()
            }),
            resistance_px: 0,
        }])
        .await;
    let driven = visibility(&mut fake);
    daemon.input_actions(vec![Action::EndedDriven]).await;

    // Assert
    assert_eq!(driven, [false]);
    assert!(matches!(fake.inject.try_recv(), Ok(Inject::Warp { .. })));
    assert_eq!(visibility(&mut fake), [true]);
}
