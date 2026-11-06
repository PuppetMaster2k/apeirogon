// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_core::hotkey::HotkeyMatcher;

use super::{Action, Cursor, Daemon, EdgeResistance, Inject, MotionScale, landing_point};

impl Daemon {
    pub(in crate::daemon) async fn sync_grab(&mut self) {
        self.input
            .grab(
                self.input.focus.is_driven()
                    || matches!(self.input.focus.cursor(), Cursor::Driving(_)),
            )
            .await;
    }

    pub(in crate::daemon) async fn input_actions(&mut self, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::Send { peer, body } => self.send_input(peer, body),
                Action::Release(events) => {
                    self.input.buttons.reset();
                    for event in events {
                        self.input.inject(Inject::Event(event)).await;
                    }
                }
                Action::Cursor(cursor) => {
                    self.input.buttons.reset();
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
                    self.input.buttons.reset();
                    self.input.hotkeys = HotkeyMatcher::new(self.config.bindings());
                    self.input.tracker.reset();
                    self.input.scale = MotionScale::new(self.config.input.speed);
                    self.input.resistance = EdgeResistance::new(resistance_px);
                    self.sync_grab().await;
                    if let (Some(landing), Some(injector)) = (landing, &self.input.injector) {
                        let (x, y) = landing_point(injector.bounds, &landing);
                        self.input.inject(Inject::Warp { x, y }).await;
                    }
                }
            }
        }
    }
}
