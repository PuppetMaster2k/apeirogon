// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_platform::input::{Capture, Injector};

use super::super::Daemon;

impl Daemon {
    pub(super) fn attach_input(&mut self, (capture, injector): (Capture, Injector)) {
        let left = capture
            .monitors
            .iter()
            .map(|monitor| monitor.x)
            .min()
            .unwrap_or(0);
        let top = capture
            .monitors
            .iter()
            .map(|monitor| monitor.y)
            .min()
            .unwrap_or(0);
        self.info.monitors = capture
            .monitors
            .iter()
            .map(|monitor| {
                Some(a7n_proto::v1::MonitorRect {
                    x: monitor.x.checked_sub(left)?,
                    y: monitor.y.checked_sub(top)?,
                    width: monitor.width,
                    height: monitor.height,
                })
            })
            .collect::<Option<Vec<_>>>()
            .unwrap_or_default();
        self.info.discard_invalid_monitors();
        self.input.capture = Some(capture);
        self.input.injector = Some(injector);
        self.input.unavailable = None;
        self.input.failed = false;
    }
}
