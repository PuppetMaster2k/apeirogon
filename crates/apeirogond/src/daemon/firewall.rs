// SPDX-License-Identifier: GPL-3.0-or-later

//! Windows Firewall diagnostics, refreshed in the background for `status`.

use std::time::{Duration, Instant};

use a7n_ipc::FirewallProblem;
use a7n_platform::firewall::{self, CheckError};

use super::{Daemon, Event};

/// Rules and the network profile change while the daemon runs, so the answer
/// is refreshed; the daemon never touches the firewall itself.
pub(super) const FIREWALL_CHECK_INTERVAL: Duration = Duration::from_secs(30);

pub(super) struct State {
    pub problems: Vec<FirewallProblem>,
    /// When the next check starts; None while one runs, or when this platform
    /// has no firewall backend.
    pub next_check: Option<Instant>,
    failed: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            problems: Vec::new(),
            next_check: firewall::SUPPORTED.then(Instant::now),
            failed: false,
        }
    }
}

impl Daemon {
    pub(super) fn check_firewall(&mut self) {
        self.firewall.next_check = None;
        let Ok(addr) = self.endpoint.local_addr() else {
            self.firewall.next_check = Some(Instant::now() + FIREWALL_CHECK_INTERVAL);
            return;
        };
        let events = self.mailboxes.events.clone();
        self.tasks.spawn_blocking(move || {
            let result = firewall::check(addr.port())
                .map(|problems| problems.into_iter().map(Into::into).collect());
            let _ = events.blocking_send(Event::FirewallChecked(result));
        });
    }

    pub(super) fn firewall_checked(&mut self, result: Result<Vec<FirewallProblem>, CheckError>) {
        self.firewall.next_check = Some(Instant::now() + FIREWALL_CHECK_INTERVAL);
        let failed = result.is_err();
        if failed && !self.firewall.failed {
            crate::log::event!("kind=firewall_check_failed");
        }
        self.firewall.failed = failed;
        self.firewall.problems = result.unwrap_or_default();
    }
}
