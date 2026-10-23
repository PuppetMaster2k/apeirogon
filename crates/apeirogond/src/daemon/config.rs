// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

use a7n_core::hotkey::HotkeyMatcher;
use a7n_core::input::MotionScale;
use a7n_ipc::config::Config;
use a7n_proto::MachineId;
use a7n_proto::v1::{Accepts, InputEvent, envelope::Body, input_event};

use super::{Daemon, DaemonError};

impl Daemon {
    pub(crate) fn with_config(mut self, config: Config, path: PathBuf) -> Self {
        self.info.name = config
            .name
            .clone()
            .or_else(a7n_platform::host_name)
            .unwrap_or_else(|| self.identity.machine_id.to_string());
        self.input.scale = MotionScale::new(config.input.speed);
        self.input.hotkeys = HotkeyMatcher::new(config.bindings());
        self.config = config;
        self.config_path = path;
        self
    }

    pub(super) fn allow_drive(&self, peer: MachineId) -> bool {
        self.input.injector.is_some()
            && self.pins.get(&peer).is_some()
            && self
                .config
                .peer(&peer)
                .is_none_or(|policy| policy.allow_drive)
    }

    pub(super) async fn reload_config(&mut self) -> Result<(), DaemonError> {
        let config = Config::load(&self.config_path)?;
        let before: Vec<_> = self
            .links
            .by_peer
            .keys()
            .map(|peer| (*peer, self.allow_drive(*peer)))
            .collect();
        if config.input.speed.to_bits() != self.config.input.speed.to_bits() {
            self.input.scale = MotionScale::new(config.input.speed);
        }
        self.input.hotkeys = HotkeyMatcher::new(config.bindings());
        self.config = config;
        for (peer, was_allowed) in before {
            let accepts = self.allow_drive(peer);
            if accepts == was_allowed {
                continue;
            }
            if !accepts {
                let actions = self.input.focus.deny_drive(peer);
                self.input_actions(actions).await;
                self.sync_grab().await;
            }
            self.send_input(peer, Body::Accepts(Accepts { accepts }));
        }
        Ok(())
    }

    pub(super) fn snapshot_remap(&mut self, peer: MachineId) {
        self.input.modifier_remap =
            self.config
                .peer(&peer)
                .map_or_else(Default::default, |policy| {
                    policy
                        .modifier_remap
                        .iter()
                        .map(|(from, to)| (from.usage(), to.usage()))
                        .collect()
                });
    }

    pub(super) fn transform_input(&self, event: &mut InputEvent) {
        match &mut event.event {
            Some(input_event::Event::Key(key)) => {
                if let Some(usage) = self.input.modifier_remap.get(&key.usage) {
                    key.usage = *usage;
                }
            }
            Some(input_event::Event::Scroll(scroll)) if self.config.input.natural_scrolling => {
                scroll.dx = -scroll.dx;
                scroll.dy = -scroll.dy;
            }
            _ => {}
        }
    }
}
