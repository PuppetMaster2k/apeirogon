// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use a7n_core::hotkey::{HotkeyMatcher, recorded_chord, recorded_modifiers, recorded_trigger};
use a7n_ipc::HotkeyKind;
use a7n_proto::v1::{KeyEvent, input_event::Event};

use super::{Captured, Daemon, DaemonError, now};
use crate::command::Reply;

const RECORD_TIMEOUT: Duration = Duration::from_secs(10);

struct Pending {
    deadline: Instant,
    kind: HotkeyKind,
    reply: Reply<String>,
    keys: BTreeSet<u32>,
}

#[derive(Default)]
pub(in crate::daemon) struct Recording {
    pending: Option<Pending>,
    suppressed: BTreeSet<u32>,
}

impl Recording {
    pub(in crate::daemon) fn next_deadline(&self) -> Option<Instant> {
        self.pending.as_ref().map(|pending| pending.deadline)
    }

    pub(in crate::daemon) fn expire(&mut self, now: Instant) {
        if self.next_deadline().is_some_and(|deadline| now >= deadline) {
            self.finish();
        }
    }

    fn finish(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let keys: Vec<_> = pending.keys.into_iter().collect();
        let result = if keys.is_empty() {
            Err(DaemonError::InvalidRequest("no key pressed"))
        } else {
            match pending.kind {
                HotkeyKind::Trigger => recorded_trigger(&keys),
                HotkeyKind::Chord => recorded_chord(&keys),
                HotkeyKind::Modifiers => recorded_modifiers(&keys),
            }
            .map_err(DaemonError::Hotkey)
        };
        let _ = pending.reply.send(result);
    }

    fn on_key(&mut self, key: &KeyEvent) -> bool {
        if !key.down && self.suppressed.remove(&key.usage) {
            if self.suppressed.is_empty() {
                self.finish();
            }
            return true;
        }
        // A timeout or disconnected client must still consume held keys through release.
        if self.suppressed.contains(&key.usage) {
            return true;
        }
        let Some(pending) = &mut self.pending else {
            return false;
        };
        if !key.down || pending.reply.is_closed() {
            return false;
        }
        pending.keys.insert(key.usage);
        self.suppressed.insert(key.usage);
        true
    }
}

impl Daemon {
    pub(in crate::daemon) fn record_hotkey(&mut self, kind: HotkeyKind, reply: Reply<String>) {
        if self
            .input
            .recording
            .pending
            .as_ref()
            .is_some_and(|pending| !pending.reply.is_closed())
            || !self.input.recording.suppressed.is_empty()
        {
            let _ = reply.send(Err(DaemonError::InvalidRequest(
                "hotkey recording already active",
            )));
            return;
        }
        if self.input.capture.is_none() {
            let _ = reply.send(Err(DaemonError::InvalidRequest("no key pressed")));
            return;
        }
        self.input.hotkeys = HotkeyMatcher::new(self.config.bindings());
        self.input.recording.pending = Some(Pending {
            deadline: now() + RECORD_TIMEOUT,
            kind,
            reply,
            keys: BTreeSet::new(),
        });
    }

    pub(super) fn record_capture(&mut self, event: &Captured) -> bool {
        self.input.recording.expire(now());
        let Captured::Input(input) = event else {
            return false;
        };
        let Some(Event::Key(key)) = &input.event else {
            return false;
        };
        self.input.recording.on_key(key)
    }
}

#[cfg(test)]
mod tests;
