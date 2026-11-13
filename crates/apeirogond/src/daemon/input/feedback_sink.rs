// SPDX-License-Identifier: GPL-3.0-or-later

//! Decides which cues the feedback backend gets; backends stay dumb.

use a7n_core::focus::edge::quarter_crossed;
use a7n_ipc::config::FeedbackConfig;
use a7n_platform::feedback::{Command, Feedback};
use a7n_platform::input::Bounds;
use a7n_proto::v1::Edge;

/// Smallest progress change worth another edge command, in per-mille.
const FEEDBACK_STEP_PERMILLE: u16 = 50;
const FEEDBACK_FULL_PERMILLE: u16 = 1000;

#[derive(Debug, Default)]
pub(in crate::daemon) struct FeedbackSink {
    backend: Option<Feedback>,
    config: FeedbackConfig,
    /// What the backend last drew; `Some` means a `Clear` is owed.
    last: Option<(Bounds, Edge, u16)>,
}

impl FeedbackSink {
    pub(super) fn attach(&mut self, backend: Option<Feedback>) {
        self.backend = backend;
    }

    pub(in crate::daemon) fn configure(&mut self, config: FeedbackConfig) {
        let changed = self.config.look != config.look;
        if !config.edge {
            self.clear();
        }
        self.config = config;
        if config.edge
            && changed
            && let Some((display, edge, progress)) = self.last
        {
            self.send(Command::Edge {
                display,
                edge,
                progress,
                look: config.look,
            });
        }
    }

    /// Reports the current push; sends only when it moved by a full step or
    /// changed edge, and ticks when it passes a quarter.
    pub(super) fn push(&mut self, display: Bounds, progress: Option<(Edge, u16)>) {
        let Some((edge, value)) = progress else {
            self.clear();
            return;
        };
        let previous = self
            .last
            .filter(|(last_display, last_edge, _)| *last_display == display && *last_edge == edge);
        let from = previous.map_or(0, |(_, _, last_value)| last_value);
        if previous.is_some() && value.abs_diff(from) < FEEDBACK_STEP_PERMILLE {
            return;
        }
        self.last = Some((display, edge, value));
        if self.config.edge {
            self.send(Command::Edge {
                display,
                edge,
                progress: value,
                look: self.config.look,
            });
        }
        if self.config.haptics && quarter_crossed(from, value) {
            self.send(Command::Tick);
        }
    }

    pub(super) fn fade(&mut self, progress: Option<(Edge, u16)>) {
        if let Some((display, _, _)) = self.last {
            self.push(display, progress);
        } else {
            self.clear();
        }
    }

    pub(super) fn crossed(&mut self, display: Bounds, edge: Edge) {
        // A single large motion may cross before the first pressure frame.
        self.push(display, Some((edge, FEEDBACK_FULL_PERMILLE)));
        if self.last.take().is_some() && self.config.edge {
            self.send(Command::Crossed);
        }
    }

    pub(super) fn clear(&mut self) {
        if self.last.take().is_some() && self.config.edge {
            self.send(Command::Clear);
        }
    }

    pub(super) fn landing(&self, x: i32, y: i32) {
        if self.config.landing {
            self.send(Command::Landing {
                x,
                y,
                look: self.arrival_look(),
            });
        }
    }

    pub(super) fn landing_at_cursor(&self) {
        if self.config.landing {
            self.send(Command::LandingAtCursor {
                look: self.arrival_look(),
            });
        }
    }

    fn arrival_look(&self) -> a7n_platform::feedback::Look {
        a7n_platform::feedback::Look {
            show_edge: self.config.edge,
            show_marker: self.config.landing,
            ..self.config.look
        }
    }

    fn send(&self, command: Command) {
        if let Some(backend) = &self.backend {
            backend.send(command);
        }
    }
}
