// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::{Duration, Instant};

use a7n_core::hotkey::{HotkeyMatcher, trigger_name};
use a7n_proto::v1::input_event::Event;

use super::{Captured, Daemon, DaemonError, now};
use crate::command::Reply;

const RECORD_TIMEOUT: Duration = Duration::from_secs(10);
const INVALID_TRIGGER: &str =
    "that key types text; pick a modifier, F-key, Scroll Lock, Pause or Menu";

#[derive(Default)]
pub(in crate::daemon) struct Recording {
    pending: Option<(Instant, Reply<String>)>,
    suppressed: Option<u32>,
}

impl Recording {
    pub(in crate::daemon) fn next_deadline(&self) -> Option<Instant> {
        self.pending.as_ref().map(|(deadline, _)| *deadline)
    }

    pub(in crate::daemon) fn expire(&mut self, now: Instant) {
        if self.next_deadline().is_some_and(|deadline| now >= deadline) {
            if let Some((_, reply)) = self.pending.take() {
                let _ = reply.send(Err(DaemonError::InvalidRequest("no key pressed")));
            }
        }
    }
}

impl Daemon {
    pub(in crate::daemon) fn record_trigger_key(&mut self, reply: Reply<String>) {
        if self
            .input
            .recording
            .pending
            .as_ref()
            .is_some_and(|(_, reply)| !reply.is_closed())
            || self.input.recording.suppressed.is_some()
        {
            let _ = reply.send(Err(DaemonError::InvalidRequest(
                "trigger recording already active",
            )));
            return;
        }
        if self.input.capture.is_none() {
            let _ = reply.send(Err(DaemonError::InvalidRequest("no key pressed")));
            return;
        }
        self.input.hotkeys = HotkeyMatcher::new(self.config.bindings());
        self.input.recording.pending = Some((now() + RECORD_TIMEOUT, reply));
    }

    pub(super) fn record_capture(&mut self, event: &Captured) -> bool {
        self.input.recording.expire(now());
        let Captured::Input(input) = event else {
            return false;
        };
        let Some(Event::Key(key)) = &input.event else {
            return false;
        };
        let recording = &mut self.input.recording;
        if recording.suppressed == Some(key.usage) {
            if !key.down {
                recording.suppressed = None;
            }
            return true;
        }
        if !key.down
            || recording
                .pending
                .as_ref()
                .is_none_or(|(_, reply)| reply.is_closed())
        {
            return false;
        }
        if let Some((_, reply)) = recording.pending.take() {
            let result = trigger_name(key.usage)
                .map(str::to_owned)
                .ok_or(DaemonError::InvalidRequest(INVALID_TRIGGER));
            let _ = reply.send(result);
            recording.suppressed = Some(key.usage);
        }
        true
    }
}
