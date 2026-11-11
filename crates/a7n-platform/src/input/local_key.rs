// SPDX-License-Identifier: GPL-3.0-or-later

/// Preserve local key releases across capture grab changes.
#[derive(Clone, Copy, Default)]
pub(super) struct LocalKey {
    pub(super) held: bool,
    passed: bool,
}

impl LocalKey {
    pub(super) fn swallow(&mut self, down: bool, grabbed: bool) -> bool {
        if down {
            if self.held && self.passed {
                return false;
            }
            if !self.held {
                self.passed = !grabbed;
            }
        } else if std::mem::take(&mut self.passed) {
            self.held = false;
            return false;
        }
        self.held = down;
        grabbed
    }
}
