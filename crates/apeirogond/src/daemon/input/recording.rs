// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use a7n_core::hotkey::{HotkeyMatcher, recorded_chord, recorded_modifiers, recorded_trigger};
use a7n_core::local_input::LocalInput;
use a7n_ipc::HotkeyKind;
use a7n_proto::v1::input_event::Event;

use super::{Captured, Daemon, DaemonError, now};
use crate::command::Reply;

const RECORD_TIMEOUT: Duration = Duration::from_secs(10);

struct Pending {
    deadline: Instant,
    kind: HotkeyKind,
    reply: Reply<String>,
    keys: BTreeSet<u32>,
    single: Option<LocalInput>,
}

#[derive(Default)]
pub(in crate::daemon) struct Recording {
    pending: Option<Pending>,
    suppressed: BTreeSet<LocalInput>,
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
        let result = if keys.is_empty() && pending.single.is_none() {
            Err(DaemonError::InvalidRequest("no key pressed"))
        } else if pending.kind == HotkeyKind::Single {
            pending
                .single
                .and_then(LocalInput::name)
                .ok_or(DaemonError::InvalidRequest(
                    "pick one named key or middle, back or forward mouse button",
                ))
        } else if keys.is_empty() {
            Err(DaemonError::InvalidRequest("no key pressed"))
        } else {
            match pending.kind {
                HotkeyKind::Single => unreachable!("single inputs were handled above"),
                HotkeyKind::Trigger => recorded_trigger(&keys),
                HotkeyKind::Chord => recorded_chord(&keys),
                HotkeyKind::Modifiers => recorded_modifiers(&keys),
            }
            .map_err(DaemonError::Hotkey)
        };
        let _ = pending.reply.send(result);
    }

    fn on_key(&mut self, key: &a7n_proto::v1::KeyEvent) -> bool {
        self.on_input(LocalInput::Key(key.usage), key.down)
    }

    fn on_input(&mut self, input: LocalInput, down: bool) -> bool {
        if !down && self.suppressed.remove(&input) {
            if self.suppressed.is_empty() {
                self.finish();
            }
            return true;
        }
        // A timeout or disconnected client must still consume held keys through release.
        if self.suppressed.contains(&input) {
            return true;
        }
        let Some(pending) = &mut self.pending else {
            return false;
        };
        if !down || pending.reply.is_closed() {
            return false;
        }
        if pending.kind != HotkeyKind::Single && matches!(input, LocalInput::Button(_)) {
            return false;
        }
        if pending.kind == HotkeyKind::Single && pending.single.is_none() {
            pending.single = Some(input);
        }
        if let LocalInput::Key(usage) = input {
            pending.keys.insert(usage);
        }
        self.suppressed.insert(input);
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
            single: None,
        });
    }

    pub(super) fn record_capture(&mut self, event: &Captured) -> bool {
        self.input.recording.expire(now());
        let Captured::Input(input) = event else {
            return false;
        };
        match &input.event {
            Some(Event::Key(key)) => self.input.recording.on_key(key),
            Some(event) => LocalInput::from_event(event)
                .is_some_and(|(input, down)| self.input.recording.on_input(input, down)),
            None => false,
        }
    }
}

#[cfg(test)]
mod tests;
