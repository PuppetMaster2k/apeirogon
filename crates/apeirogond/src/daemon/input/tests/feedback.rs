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
    let own = daemon.identity.machine_id;
    let mut placements = vec![Placement {
        id: own,
        x: 0,
        y: 0,
        width: 100,
        height: 100,
        blocked: 0,
    }];
    for (byte, x, y) in [(42, 100, 0), (43, 0, 100)] {
        let id = MachineId::try_from(&[byte; MachineId::LEN][..]).unwrap();
        daemon.pins.add(Entry {
            machine_id: id,
            pin: daemon.identity.pin,
            name: "peer".into(),
            spki_der: None,
            hw_addr: None,
            screen_mm: None,
        });
        placements.push(Placement {
            id,
            x,
            y,
            width: 100,
            height: 100,
            blocked: 0,
        });
    }
    daemon.layout.state = daemon.layout.state.edit(placements, own).unwrap();
    (daemon, dir, commands)
}

fn drain(commands: &mut mpsc::Receiver<Command>) -> Vec<Command> {
    std::iter::from_fn(|| commands.try_recv().ok()).collect()
}

#[tokio::test(start_paused = true)]
async fn cross_edge_no_neighbour_sends_no_edge_feedback() {
    // Arrange
    let (mut daemon, _dir, mut commands) = daemon_with_feedback(FeedbackConfig {
        edge: true,
        landing: false,
        haptics: true,
        ..FeedbackConfig::default()
    });

    // Act
    for dx in [-30, -100] {
        daemon
            .cross_edge(DISPLAY, &[DISPLAY], (0, 50), (dx, 0), false)
            .await;
    }

    // Assert
    assert_eq!(drain(&mut commands), [] as [Command; 0]);
    assert_eq!(daemon.input.resistance.progress(), None);
    assert!(daemon.wol.pending.is_none());
}

#[tokio::test(start_paused = true)]
async fn cross_edge_inward_travel_reports_reduced_pressure() {
    // Arrange
    let config = FeedbackConfig {
        edge: true,
        landing: true,
        haptics: true,
        ..FeedbackConfig::default()
    };
    let (mut daemon, _dir, mut commands) = daemon_with_feedback(config);
    let edge = |progress| Command::Edge {
        display: DISPLAY,
        edge: Edge::Right,
        progress,
        look: feedback::Look::default(),
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
            edge(500)
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn input_actions_driven_landing_sends_landing_and_edge_off_sends_no_edge() {
    // Arrange
    let config = FeedbackConfig {
        edge: false,
        landing: true,
        haptics: false,
        look: feedback::Look {
            landing: feedback::Landing::Pulse,
            color: feedback::Color::Pink,
            size: feedback::Size::Large,
            ..feedback::Look::default()
        },
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
    assert_eq!(
        drain(&mut commands),
        [Command::Landing {
            x: 0,
            y: 49,
            look: config.look
        }]
    );
}

#[tokio::test]
async fn captured_and_pointer_feedback_inset_edge_glows_on_pushed_monitor() {
    for driven in [false, true] {
        // Arrange
        let (mut daemon, _dir, mut commands) = daemon_with_feedback(FeedbackConfig {
            edge: true,
            landing: true,
            haptics: false,
            ..FeedbackConfig::default()
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
                progress: 300,
                look: feedback::Look::default()
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
        ..FeedbackConfig::default()
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
        [Command::Landing {
            x: 1920,
            y: 1295,
            look: a7n_platform::feedback::Look::default()
        }]
    );
}

#[tokio::test(start_paused = true)]
async fn poll_edge_pressure_resting_pointer_fades_and_clears_without_haptics() {
    // Arrange
    let config = FeedbackConfig {
        edge: true,
        landing: false,
        haptics: true,
        ..FeedbackConfig::default()
    };
    let (mut daemon, _dir, mut commands) = daemon_with_feedback(config);
    daemon
        .cross_edge(DISPLAY, &[DISPLAY], (99, 50), (60, 0), false)
        .await;
    drain(&mut commands);
    let deadline = daemon.input.resistance.next_deadline().unwrap();

    // Act
    super::super::deadline(Some(deadline)).await;
    daemon.input.poll_edge_pressure();
    let faded = drain(&mut commands);
    tokio::time::advance(std::time::Duration::from_secs(1)).await;
    daemon.input.poll_edge_pressure();

    // Assert
    assert_eq!(
        faded,
        [Command::Edge {
            display: DISPLAY,
            edge: Edge::Right,
            progress: 540,
            look: feedback::Look::default()
        }]
    );
    assert_eq!(drain(&mut commands), [Command::Clear]);
    assert_eq!(daemon.input.resistance.next_deadline(), None);
}

#[tokio::test(start_paused = true)]
async fn poll_edge_pressure_inset_monitor_fades_on_pushed_display() {
    // Arrange
    let (mut daemon, _dir, mut commands) = daemon_with_feedback(FeedbackConfig {
        edge: true,
        landing: false,
        haptics: false,
        ..FeedbackConfig::default()
    });
    let laptop = Bounds::from((0, 0, 1920, 1200));
    let monitors = vec![laptop, (1920, 0, 2560, 1440).into()];
    let capture = daemon.input.capture.as_mut().unwrap();
    capture.bounds = (0, 0, 4480, 1440).into();
    capture.monitors = monitors.into();
    daemon
        .captured(Some(Captured::Pointer {
            x: 100,
            y: 1199,
            dx: 0,
            dy: 60,
        }))
        .await;
    drain(&mut commands);
    let deadline = daemon.input.resistance.next_deadline().unwrap();

    // Act
    super::super::deadline(Some(deadline)).await;
    daemon.input.poll_edge_pressure();

    // Assert
    assert_eq!(
        drain(&mut commands),
        [Command::Edge {
            display: laptop,
            edge: Edge::Bottom,
            progress: 540,
            look: feedback::Look::default()
        }]
    );
}

#[tokio::test(start_paused = true)]
async fn cross_edge_non_outward_motion_subtracts_only_inward_component() {
    // Arrange
    for (edge, x, y, dx, dy) in [
        (Edge::Left, 0, 50, 10, 40),
        (Edge::Right, 99, 50, -10, 40),
        (Edge::Top, 50, 0, 40, 10),
        (Edge::Bottom, 50, 99, 40, -10),
        (Edge::Right, 99, 50, 0, 10),
        (Edge::Right, 50, 50, 10, 0),
    ] {
        let (mut daemon, _dir, _commands) = daemon_with_feedback(FeedbackConfig::default());
        daemon.input.resistance.push(edge, 60, now());

        // Act
        daemon
            .cross_edge(DISPLAY, &[DISPLAY], (x, y), (dx, dy), false)
            .await;

        // Assert
        let expected = if edge == Edge::Right && dx >= 0 {
            600
        } else {
            500
        };
        assert_eq!(daemon.input.resistance.progress(), Some((edge, expected)));
    }
}

#[tokio::test(start_paused = true)]
async fn poll_edge_pressure_paused_or_locked_keeps_feedback_clear() {
    // Arrange
    for (paused, locked) in [(true, false), (false, true)] {
        let (mut daemon, _dir, mut commands) = daemon_with_feedback(FeedbackConfig::default());
        daemon
            .cross_edge(DISPLAY, &[DISPLAY], (99, 50), (60, 0), false)
            .await;
        drain(&mut commands);
        daemon.input.paused = paused;
        daemon.input.focus.set_locked(locked);

        // Act
        daemon
            .cross_edge(DISPLAY, &[DISPLAY], (99, 50), (1, 0), false)
            .await;
        daemon.input.poll_edge_pressure();

        // Assert
        assert_eq!(drain(&mut commands), [Command::Clear]);
    }
}
