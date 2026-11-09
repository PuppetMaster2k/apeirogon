// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_platform::input::{GESTURE_CHORDS_SUPPORTED, Inject};
use a7n_proto::MachineId;
use a7n_proto::v1::{Capability, Gesture, InputEvent, input_event::Event};

use super::Daemon;

impl Daemon {
    pub(super) fn peer_accepts_gestures(&self, peer: MachineId) -> bool {
        self.links
            .by_peer
            .get(&peer)
            .is_some_and(|link| link.capabilities.contains(&Capability::Gestures))
    }

    pub(super) async fn inject_gesture(&mut self, peer: MachineId, gesture: i32) {
        if !self.peer_accepts_gestures(peer) {
            return;
        }
        if !GESTURE_CHORDS_SUPPORTED {
            self.input
                .inject(Inject::Event(InputEvent {
                    event: Some(Event::Gesture(gesture)),
                }))
                .await;
            return;
        }
        let Ok(gesture) = Gesture::try_from(gesture) else {
            return;
        };
        let keys: Vec<_> = self.input.gestures.events(gesture).collect();
        if keys.is_empty() {
            return;
        }
        self.input.inject(Inject::Chord(keys)).await;
    }
}
