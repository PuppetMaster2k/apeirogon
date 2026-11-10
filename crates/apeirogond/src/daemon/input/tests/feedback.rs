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
    daemon.cross_edge(DISPLAY, 99, 50, 30, 0, false).await;
    daemon.cross_edge(DISPLAY, 99, 50, 30, 0, false).await;
    daemon.cross_edge(DISPLAY, 50, 50, -10, 0, false).await;

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
    daemon.cross_edge(DISPLAY, 99, 50, 30, 0, false).await;
    daemon.input_actions(vec![driven]).await;

    // Assert
    assert_eq!(drain(&mut commands), [Command::Landing { x: 0, y: 49 }]);
}
