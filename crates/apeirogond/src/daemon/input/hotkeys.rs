// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_core::hotkey::Hotkey;
use a7n_proto::v1::input_event;

use super::{Captured, Cursor, Daemon, Take, now};

impl Daemon {
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

    async fn apply_hotkey(&mut self, hotkey: Hotkey) {
        let actions = match hotkey {
            Hotkey::Home => {
                eprintln!("kind=hotkey action=home");
                self.input.focus.go_home()
            }
            Hotkey::Lock => {
                eprintln!("kind=hotkey action=lock");
                self.input.focus.toggle_lock();
                Vec::new()
            }
            Hotkey::Jump(n) => {
                eprintln!("kind=hotkey action=jump");
                let Some(target) = self.layout.state.nth(n) else {
                    eprintln!("kind=hotkey_target_unavailable");
                    return;
                };
                if target == self.identity.machine_id {
                    self.input.focus.go_home()
                } else if self.links.by_peer.contains_key(&target) {
                    self.input.focus.take(
                        Take {
                            target,
                            landing: None,
                            resistance_px: 0,
                            reach: self.links.by_peer.keys().copied().collect(),
                        },
                        now(),
                    )
                } else {
                    eprintln!("kind=hotkey_target_unavailable");
                    return;
                }
            }
        };
        self.input_actions(actions).await;
    }
}
