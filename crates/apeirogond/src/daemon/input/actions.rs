// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_core::hotkey::{HotkeyMatcher, ShakeDetector};

use super::{Action, Cursor, Daemon, EdgeResistance, Inject, MotionScale, landing_point};

impl Daemon {
    pub(in crate::daemon) async fn sync_grab(&mut self) {
        self.sync_local_inputs().await;
        self.input
            .grab(
                self.input.focus.is_driven()
                    || matches!(self.input.focus.cursor(), Cursor::Driving(_)),
            )
            .await;
    }

    pub(in crate::daemon) async fn sync_local_inputs(&mut self) {
        let inputs = if matches!(self.input.focus.cursor(), Cursor::Driving(_)) {
            self.input.local_inputs.clone()
        } else {
            Vec::new()
        };
        if let Some(capture) = &self.input.capture {
            if capture
                .control
                .send(a7n_platform::input::CaptureControl::LocalInputs(inputs))
                .await
                .is_err()
            {
                self.input.closed();
            }
        }
    }

    pub(in crate::daemon) async fn input_actions(&mut self, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::Send { peer, body } => self.send_input(peer, body),
                Action::Release(events) => {
                    self.input.reset_edge_state();
                    for event in events {
                        self.input.inject(Inject::Event(event)).await;
                    }
                }
                Action::Cursor(cursor) => {
                    self.input.shake = ShakeDetector::new(self.config.hotkeys.shake);
                    self.input.reset_edge_state();
                    if let Cursor::Driving(peer) = cursor {
                        self.input.counter.reset();
                        self.snapshot_remap(peer);
                    }
                    self.input.resistance = EdgeResistance::new(0);
                    self.sync_grab().await;
                }
                Action::Driven {
                    landing,
                    resistance_px,
                    ..
                } => {
                    self.input.reset_edge_state();
                    self.input.hotkeys = HotkeyMatcher::new(self.config.bindings());
                    self.input.shake = ShakeDetector::new(self.config.hotkeys.shake);
                    self.input.tracker.reset();
                    self.input.scale = MotionScale::new(self.config.input.speed);
                    self.input.resistance = EdgeResistance::new(resistance_px);
                    self.sync_grab().await;
                    if let (Some(landing), Some(injector)) = (landing, &self.input.injector) {
                        let (x, y) = landing_point(injector.bounds, &injector.monitors, &landing);
                        self.input.inject(Inject::Warp { x, y }).await;
                        self.input.sink.landing(x, y);
                    }
                }
            }
        }
    }
}
