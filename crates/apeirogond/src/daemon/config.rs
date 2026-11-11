// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeMap;
use std::path::PathBuf;

use a7n_core::focus::edge::FullscreenHold;
use a7n_core::hotkey::{HotkeyMatcher, ShakeDetector};
use a7n_core::input::{MotionScale, ScrollScale};
use a7n_core::modifiers::default_modifier_remap;
use a7n_ipc::config::{Config, MAX_SPEED, MIN_SPEED, ModifierMode};
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
        self.input.idle_cursor = a7n_core::cursor::IdleCursor::new(config.input.hide_idle_cursor);
        self.input.scale = MotionScale::new(config.input.speed);
        self.input.fullscreen_hold = FullscreenHold::new(config.input.fullscreen_hold);
        self.input.sink.configure(config.feedback);
        self.input.hotkeys = HotkeyMatcher::new(config.bindings());
        self.input.gestures = config.gesture_bindings();
        self.input.shake = ShakeDetector::new(config.hotkeys.shake);
        self.input.local_inputs = config
            .local_inputs()
            .expect("local input settings have been validated");
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
        let enabling_sync = !self.config.sync.settings && config.sync.settings;
        self.apply_config(config).await;
        self.detect_settings_edit()?;
        if enabling_sync {
            self.broadcast_settings();
        }
        Ok(())
    }

    pub(super) async fn apply_config(&mut self, config: Config) {
        let change = self
            .input
            .idle_cursor
            .configure(config.input.hide_idle_cursor);
        self.input.cursor_visibility(change).await;
        let before: Vec<_> = self
            .links
            .by_peer
            .keys()
            .map(|peer| (*peer, self.allow_drive(*peer)))
            .collect();
        let driver = self.input.focus.driven_by();
        let scales_changed = driver.is_some_and(|peer| {
            Self::input_speeds(&config, peer) != Self::input_speeds(&self.config, peer)
        });
        if config.input.fullscreen_hold != self.config.input.fullscreen_hold {
            self.input.fullscreen_hold = FullscreenHold::new(config.input.fullscreen_hold);
        }
        self.input.sink.configure(config.feedback);
        self.input.hotkeys = HotkeyMatcher::new(config.bindings());
        self.input.gestures = config.gesture_bindings();
        self.input.shake = ShakeDetector::new(config.hotkeys.shake);
        self.input.local_inputs = config
            .local_inputs()
            .expect("local input settings have been validated");
        self.config = config;
        if scales_changed {
            self.configure_input_scales(driver.expect("scale changes require an active driver"));
        }
        self.sync_local_inputs().await;
        self.recheck_files_policy();
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
    }

    fn input_speeds(config: &Config, peer: MachineId) -> (f64, f64, bool) {
        let policy = config.peer(&peer);
        (
            (config.input.speed * policy.map_or(1.0, |p| p.pointer_speed))
                .clamp(MIN_SPEED, MAX_SPEED),
            policy.map_or(1.0, |p| p.scroll_speed),
            policy.is_some_and(|p| p.reverse_scroll),
        )
    }

    pub(super) fn configure_input_scales(&mut self, peer: MachineId) {
        let (pointer, scroll, reverse) = Self::input_speeds(&self.config, peer);
        self.input.scale = MotionScale::new(pointer);
        self.input.scroll_scale = ScrollScale::new(scroll, reverse);
    }

    pub(super) fn snapshot_remap(&mut self, peer: MachineId) {
        let policy = self.config.peer(&peer);
        self.input.modifier_remap = match policy.map_or(ModifierMode::Auto, |p| p.modifier_mode) {
            ModifierMode::Auto => {
                let platform = self
                    .links
                    .by_peer
                    .get(&peer)
                    .map_or_else(a7n_proto::v1::Platform::default, |link| link.platform);
                default_modifier_remap(self.info.platform(), platform)
                    .iter()
                    .copied()
                    .collect()
            }
            ModifierMode::Off => BTreeMap::new(),
            ModifierMode::Custom => policy
                .into_iter()
                .flat_map(|p| &p.modifier_remap)
                .map(|(from, to)| (from.usage(), to.usage()))
                .collect(),
        };
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
