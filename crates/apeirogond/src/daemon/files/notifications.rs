// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::{Duration, Instant};

use super::{Daemon, Failure, MachineId, Progress, Result, State};

const OFFER_NOTIFY_INTERVAL: Duration = Duration::from_secs(10);
const NOTIFY_MIN_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(super) enum Notice {
    Offered,
    Receiving,
    Finished(Result<()>),
}

impl Daemon {
    pub(super) fn notify_files(&mut self, peer: MachineId, progress: &Progress, notice: Notice) {
        if !self.config.notifications.files
            || (matches!(notice, Notice::Offered)
                && !self.files.admit_offered_notification(peer, Instant::now()))
        {
            return;
        }
        let peer = self
            .peer_name(peer)
            .map_or_else(|| peer.to_string(), str::to_owned);
        if let Some(body) = message(&peer, progress.files_total, progress.bytes_total, notice) {
            a7n_platform::notify::notify("Apeirogon files", &body);
        }
    }
}

impl State {
    fn admit_offered_notification(&mut self, peer: MachineId, now: Instant) -> bool {
        if self
            .offered_notifications
            .get(&peer)
            .is_some_and(|last| now.duration_since(*last) < OFFER_NOTIFY_INTERVAL)
        {
            return false;
        }
        self.offered_notifications.insert(peer, now);
        true
    }
}

fn message(peer: &str, count: u32, total: u64, notice: Notice) -> Option<String> {
    if !matches!(notice, Notice::Offered) && total < NOTIFY_MIN_BYTES {
        return None;
    }
    let size = a7n_ipc::format_size(total);
    let suffix = if count == 1 { "" } else { "s" };
    let files = format!("{count} file{suffix} ({size})");
    Some(match notice {
        Notice::Offered => format!(
            "{peer} wants to send {files}. Accept in Apeirogon or with `a7n files accept {peer}`."
        ),
        Notice::Receiving => format!("Receiving {files} from {peer}"),
        Notice::Finished(Ok(())) => format!("Received {files} from {peer}"),
        Notice::Finished(Err(reason)) => {
            let reason = match reason {
                Failure::NoSpace => "not enough free space in the receive directory",
                Failure::Integrity => "integrity check failed",
                Failure::Invalid => "invalid transfer",
                Failure::Io => "I/O error",
                Failure::Cancelled => "cancelled",
                Failure::LinkLost => "connection lost",
            };
            format!("Transfer from {peer} failed: {reason} ({files})")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admit_offered_notification_repeated_peer_waits_for_interval() {
        // Arrange
        let mut state = State::default();
        let peer = MachineId::try_from(&[1; MachineId::LEN][..]).unwrap();
        let now = Instant::now();

        // Act
        let first = state.admit_offered_notification(peer, now);
        let second = state.admit_offered_notification(peer, now + OFFER_NOTIFY_INTERVAL / 2);
        let later = state.admit_offered_notification(peer, now + OFFER_NOTIFY_INTERVAL);

        // Assert
        assert!(first);
        assert!(!second);
        assert!(later);
    }
}
