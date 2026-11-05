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

    #[test]
    fn message_transfer_notices_apply_threshold_and_show_only_metadata() {
        // Arrange
        let cases = [
            (NOTIFY_MIN_BYTES - 1, Notice::Receiving, None),
            (NOTIFY_MIN_BYTES - 1, Notice::Finished(Ok(())), None),
            (
                NOTIFY_MIN_BYTES - 1,
                Notice::Finished(Err(Failure::Io)),
                None,
            ),
            (
                NOTIFY_MIN_BYTES,
                Notice::Receiving,
                Some("Receiving 3 files (100.0 MiB) from office-pc"),
            ),
            (
                NOTIFY_MIN_BYTES + 1,
                Notice::Finished(Ok(())),
                Some("Received 3 files (100.0 MiB) from office-pc"),
            ),
            (
                12,
                Notice::Offered,
                Some(
                    "office-pc wants to send 3 files (12 B). Accept in Apeirogon or with `a7n files accept office-pc`.",
                ),
            ),
            (
                NOTIFY_MIN_BYTES,
                Notice::Finished(Err(Failure::NoSpace)),
                Some(
                    "Transfer from office-pc failed: not enough free space in the receive directory (3 files (100.0 MiB))",
                ),
            ),
            (
                NOTIFY_MIN_BYTES,
                Notice::Finished(Err(Failure::Integrity)),
                Some(
                    "Transfer from office-pc failed: integrity check failed (3 files (100.0 MiB))",
                ),
            ),
            (
                NOTIFY_MIN_BYTES,
                Notice::Finished(Err(Failure::Cancelled)),
                Some("Transfer from office-pc failed: cancelled (3 files (100.0 MiB))"),
            ),
            (
                NOTIFY_MIN_BYTES,
                Notice::Finished(Err(Failure::LinkLost)),
                Some("Transfer from office-pc failed: connection lost (3 files (100.0 MiB))"),
            ),
            (
                NOTIFY_MIN_BYTES,
                Notice::Finished(Err(Failure::Invalid)),
                Some("Transfer from office-pc failed: invalid transfer (3 files (100.0 MiB))"),
            ),
            (
                NOTIFY_MIN_BYTES,
                Notice::Finished(Err(Failure::Io)),
                Some("Transfer from office-pc failed: I/O error (3 files (100.0 MiB))"),
            ),
        ];
        for (total, notice, expected) in cases {
            // Act
            let body = message("office-pc", 3, total, notice);

            // Assert
            // Exact wording also excludes any additional file names or paths.
            assert_eq!(body.as_deref(), expected);
        }
    }
}
