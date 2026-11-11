// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::config::FeedbackConfig;
use a7n_platform::feedback::{self, Command};
use a7n_proto::v1::Landing;

use super::*;

const DISPLAY: Bounds = Bounds {
    x: 0,
    y: 0,
    width: 100,
    height: 100,
};

fn daemon_with_feedback(
    config: FeedbackConfig,
) -> (Daemon, TempDir, mpsc::Receiver<feedback::Command>) {
    let (mut daemon, dir, _input) = daemon(true);
    let (sink, commands) = feedback::channel();
    daemon.input.sink.attach(Some(sink));
    daemon.input.sink.configure(config);
    daemon.input.resistance = EdgeResistance::new(100);
    (daemon, dir, commands)
}

fn drain(commands: &mut mpsc::Receiver<Command>) -> Vec<Command> {
    std::iter::from_fn(|| commands.try_recv().ok()).collect()
}

#[tokio::test]
async fn cross_edge_push_reports_rising_progress_then_clears_when_it_stops() {
    // Arrange
    let config = FeedbackConfig {
        edge: true,
        landing: true,
        haptics: true,
    };
    let (mut daemon, _dir, mut commands) = daemon_with_feedback(config);
    let edge = |progress| Command::Edge {
        display: DISPLAY,
        edge: Edge::Right,
        progress,
    };

    // Act
    daemon
        .cross_edge(DISPLAY, &[DISPLAY], (99, 50), (30, 0), false)
        .await;
    daemon
        .cross_edge(DISPLAY, &[DISPLAY], (99, 50), (30, 0), false)
        .await;
    daemon
        .cross_edge(DISPLAY, &[DISPLAY], (50, 50), (-10, 0), false)
        .await;

    // Assert
    assert_eq!(
        drain(&mut commands),
        [
            edge(300),
            Command::Tick,
            edge(600),
            Command::Tick,
            Command::Clear
        ]
    );
}

#[tokio::test]
async fn input_actions_driven_landing_sends_landing_and_edge_off_sends_no_edge() {
    // Arrange
    let config = FeedbackConfig {
        edge: false,
        landing: true,
        haptics: false,
    };
    let (mut daemon, _dir, mut commands) = daemon_with_feedback(config);
    let landing = Landing {
        edge: Edge::Left.into(),
        offset: 0.5,
        ..Default::default()
    };
    let driven = Action::Driven {
        owner: daemon.identity.machine_id,
        landing: Some(landing),
        resistance_px: 100,
    };

    // Act
    daemon
        .cross_edge(DISPLAY, &[DISPLAY], (99, 50), (30, 0), false)
        .await;
    daemon.input_actions(vec![driven]).await;

    // Assert
    assert_eq!(drain(&mut commands), [Command::Landing { x: 0, y: 49 }]);
}

#[tokio::test]
async fn captured_and_pointer_feedback_inset_edge_glows_on_pushed_monitor() {
    for driven in [false, true] {
        // Arrange
        let (mut daemon, _dir, mut commands) = daemon_with_feedback(FeedbackConfig {
            edge: true,
            landing: true,
            haptics: false,
        });
        let laptop = Bounds::from((0, 0, 1920, 1200));
        let monitors = vec![laptop, (1920, 0, 2560, 1440).into()];
        let bounds = Bounds::from((0, 0, 4480, 1440));
        if driven {
            let injector = daemon.input.injector.as_mut().unwrap();
            injector.bounds = bounds;
            injector.monitors = monitors.into();
            let peer = MachineId::try_from(&[42; MachineId::LEN][..]).unwrap();
            daemon.input.focus.on_focus(
                peer,
                &a7n_proto::v1::Focus {
                    route: 1,
                    target: daemon.identity.machine_id.as_bytes().to_vec(),
                    ..Default::default()
                },
                true,
                now(),
            );
        } else {
            let capture = daemon.input.capture.as_mut().unwrap();
            capture.bounds = bounds;
            capture.monitors = monitors.into();
        }
        let event = Some(Captured::Pointer {
            x: 100,
            y: 1199,
            dx: 0,
            dy: 30,
        });

        // Act
        if driven {
            daemon.pointer_feedback(event).await;
        } else {
            daemon.captured(event).await;
        }

        // Assert
        assert_eq!(
            drain(&mut commands),
            [Command::Edge {
                display: laptop,
                edge: Edge::Bottom,
                progress: 300
            }]
        );
    }
}

#[tokio::test]
async fn input_actions_bbox_landing_projects_warp_and_mark_onto_monitor() {
    // Arrange
    let (mut daemon, _dir, input) = daemon(true);
    let mut input = input.unwrap();
    let (sink, mut commands) = feedback::channel();
    daemon.input.sink.attach(Some(sink));
    daemon.input.sink.configure(FeedbackConfig {
        edge: false,
        landing: true,
        haptics: false,
    });
    let injector = daemon.input.injector.as_mut().unwrap();
    injector.bounds = (0, 0, 4480, 1440).into();
    injector.monitors = vec![(0, 0, 1920, 1200).into(), (1920, 0, 2560, 1440).into()].into();
    let action = Action::Driven {
        owner: daemon.identity.machine_id,
        landing: Some(Landing {
            edge: Edge::Left.into(),
            offset: 0.9,
            ..Default::default()
        }),
        resistance_px: 100,
    };

    // Act
    daemon.input_actions(vec![action]).await;

    // Assert
    assert!(matches!(
        input.inject.try_recv().unwrap(),
        Inject::Warp { x: 1920, y: 1295 }
    ));
    assert_eq!(
        drain(&mut commands),
        [Command::Landing { x: 1920, y: 1295 }]
    );
}
