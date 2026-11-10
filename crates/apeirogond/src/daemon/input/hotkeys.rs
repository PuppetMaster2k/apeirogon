// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_core::hotkey::Hotkey;
use a7n_proto::v1::input_event;

use super::{Captured, Cursor, Daemon, now};

impl Daemon {
    pub(super) async fn match_shake(&mut self, event: &Captured) -> bool {
        let Captured::Pointer { dx, .. } = event else {
            return false;
        };
        if !self.input.shake.on_motion(*dx, now()) {
            return false;
        }
        self.wol.pending = None;
        crate::log::event!("kind=hotkey action=shake");
        let actions = self.input.focus.go_home();
        self.input_actions(actions).await;
        true
    }

    pub(super) async fn match_hotkey(&mut self, event: &Captured) -> bool {
        if self.input.focus.is_driven() {
            return false;
        }
        let Captured::Input(input) = event else {
            return false;
        };
        let Some(input_event::Event::Key(key)) = &input.event else {
            return false;
        };
        let verdict = self.input.hotkeys.on_key(key.usage, key.down, now());
        let Some(action) = verdict.action else {
            return !verdict.forward;
        };
        // Forward the second double-tap press before let-go releases held keys.
        if verdict.forward {
            if let Cursor::Driving(peer) = self.input.focus.cursor() {
                self.forward_capture(peer, Captured::Input(input.clone()));
            }
        }
        // At home capture is ungrabbed, so the local OS still sees hotkey chords.
        self.apply_hotkey(action).await;
        true
    }

    pub(in crate::daemon) async fn poll_hotkey(&mut self) {
        if !self.input.focus.is_driven() {
            if let Some(action) = self.input.hotkeys.poll(now()) {
                self.apply_hotkey(action).await;
            }
        }
    }

    async fn apply_hotkey(&mut self, hotkey: Hotkey) {
        let actions = match hotkey {
            Hotkey::Home => {
                self.wol.pending = None;
                crate::log::event!("kind=hotkey action=home");
                self.input.focus.go_home()
            }
            Hotkey::Lock => {
                self.wol.pending = None;
                crate::log::event!("kind=hotkey action=lock");
                self.input.focus.toggle_lock();
                Vec::new()
            }
            Hotkey::Jump(n) => {
                crate::log::event!("kind=hotkey action=jump");
                let Some(target) = self.layout.state.nth(n, |id| self.is_layout_target(id)) else {
                    crate::log::event!("kind=hotkey_target_unavailable");
                    return;
                };
                if self.switch_machine(target).await.is_err() {
                    crate::log::event!("kind=hotkey_target_unavailable");
                }
                return;
            }
        };
        self.input_actions(actions).await;
    }

    pub(crate) async fn set_paused(&mut self, paused: bool) {
        if paused {
            self.wol.pending = None;
            let actions = self.input.focus.go_home();
            self.input_actions(actions).await;
        }
        self.input.paused = paused;
    }

    pub(crate) async fn switch_machine(
        &mut self,
        target: a7n_proto::MachineId,
    ) -> Result<(), crate::daemon::DaemonError> {
        self.wol.pending = None;
        if self.input.paused && target != self.identity.machine_id {
            return Err(crate::daemon::DaemonError::Paused);
        }
        let actions = if target == self.identity.machine_id {
            self.input.focus.go_home()
        } else if self.is_linked(target) {
            self.take_peer(target, None, now())
        } else {
            self.auto_wake(target, crate::daemon::wol::WakeAction::Switch, now())?;
            self.input.focus.go_home()
        };
        self.input_actions(actions).await;
        Ok(())
    }
}

impl Daemon {
    /// Takes the peer at once; edge resistance was already overcome or does not apply.
    pub(in crate::daemon) fn take_peer(
        &mut self,
        target: a7n_proto::MachineId,
        landing: Option<a7n_proto::v1::Landing>,
        now: std::time::Instant,
    ) -> Vec<super::Action> {
        self.input.focus.take(
            super::Take {
                target,
                landing,
                resistance_px: 0,
                reach: self.links.by_peer.keys().copied().collect(),
            },
            now,
        )
    }
}
