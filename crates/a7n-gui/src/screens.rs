// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::{IpAddr, SocketAddr};
use std::time::Instant;

use a7n_ipc::config::Modifier;
use a7n_ipc::{DEFAULT_UDP_PORT, PairingCode, Request};
use eframe::egui;

use super::{App, Command, Screen};

impl App {
    pub(super) fn machines(&mut self, ui: &mut egui::Ui) {
        let Some(status) = self.status.clone() else {
            return;
        };
        ui.heading("Machines");
        ui.label(format!(
            "{} — this machine ({})",
            status.name, status.machine
        ));
        if let Some(reason) = &status.input {
            ui.colored_label(
                egui::Color32::YELLOW,
                format!("Input unavailable: {reason}"),
            );
        }
        if ui
            .button(if status.paused {
                "Resume input sharing"
            } else {
                "Pause input sharing"
            })
            .clicked()
        {
            self.send(Command::Request(Request::SetPaused {
                paused: !status.paused,
            }));
        }
        for peer in &status.peers {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{} — {} ({})",
                    peer.name,
                    if peer.connected {
                        "connected"
                    } else {
                        "offline"
                    },
                    peer.id
                ));
                if ui.button(format!("Unpair {}", peer.name)).clicked() {
                    self.send(Command::Request(Request::Unpair { machine: peer.id }));
                }
            });
        }
        ui.heading("Nearby machines");
        for nearby in &status.nearby {
            if ui
                .button(format!("Pair with {} at {}", nearby.id, nearby.addr))
                .clicked()
            {
                self.address = nearby.addr.to_string();
            }
        }
        ui.separator();
        self.pairing_ui(ui);
    }

    fn pairing_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Pair a machine");
        if ui.button("Host pairing — show code").clicked() {
            self.send(Command::Request(Request::PairHost));
        }
        if let Some((code, expiry)) = &self.pairing {
            let remaining = expiry.saturating_duration_since(Instant::now()).as_secs();
            if remaining == 0 {
                ui.label("Pairing code expired");
            } else {
                ui.label(format!("Code: {code} — expires in {remaining} s"));
            }
        }
        ui.horizontal(|ui| {
            let label = ui.label("Address (IP or IP:port)");
            ui.text_edit_singleline(&mut self.address)
                .labelled_by(label.id);
        });
        ui.horizontal(|ui| {
            let label = ui.label("Pairing code");
            ui.text_edit_singleline(&mut self.code)
                .labelled_by(label.id);
        });
        let address = parse_address(&self.address);
        let code = PairingCode::parse(self.code.trim());
        if !self.address.is_empty() && address.is_err() {
            ui.colored_label(
                egui::Color32::RED,
                "Enter an IP or IP:port; use [IPv6]:port",
            );
        }
        if !self.code.is_empty() && code.is_err() {
            ui.colored_label(egui::Color32::RED, "Code must be six ASCII digits");
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    address.is_ok() && code.is_ok(),
                    egui::Button::new("Join pairing"),
                )
                .clicked()
            {
                if let (Ok(addr), Ok(code)) = (address, code) {
                    self.send(Command::Request(Request::PairJoin { addr, code }));
                    self.code.clear();
                }
            }
            if ui
                .add_enabled(
                    address.is_ok(),
                    egui::Button::new("Connect to paired address"),
                )
                .clicked()
            {
                if let Ok(addr) = address {
                    self.send(Command::Request(Request::Connect { addr }));
                }
            }
        });
    }

    pub(super) fn settings(&mut self, ui: &mut egui::Ui) {
        ui.heading(if self.screen == Screen::Hotkeys {
            "Hotkeys"
        } else {
            "Peer policies"
        });
        if self.edits.is_empty() {
            self.initialize_edits();
        }
        let Some(config) = &self.config else {
            if ui.button("Reload config").clicked() {
                self.send(Command::Config);
            }
            return;
        };
        if self.screen == Screen::Hotkeys {
            ui.label("Home: comma-separated chords. Jump: modifiers for machine keys 1–9.");
        } else {
            ui.label("allow_drive: true/false. Clipboard: both, send, receive, off. Modifier remaps use physical modifier names; mapping a modifier to itself restores normal behavior.");
        }
        for (key, value) in &mut self.edits {
            if let Some(status) = &self.status {
                if let Some(peer) = status
                    .peers
                    .iter()
                    .find(|p| key == &format!("peers.{}.allow_drive", p.id))
                {
                    ui.heading(&peer.name);
                }
            }
            ui.horizontal(|ui| {
                let label = ui.label(key.as_str());
                ui.text_edit_singleline(value).labelled_by(label.id);
            });
        }
        let mut candidate = config.clone();
        let validation = candidate.set_many(
            self.edits
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        );
        if let Err(error) = &validation {
            ui.colored_label(egui::Color32::RED, error.to_string());
        }
        ui.label("Saving rewrites config.toml without comments, just like a7n config set.");
        if ui
            .add_enabled(
                validation.is_ok() && !self.edits.is_empty(),
                egui::Button::new("Save settings"),
            )
            .clicked()
        {
            self.send(Command::SaveConfig(self.edits.clone()));
        }
        if ui.button("Reload config (discard edits)").clicked() {
            self.send(Command::Config);
        }
    }
    fn initialize_edits(&mut self) {
        let Some(config) = &self.config else {
            return;
        };
        if self.screen == Screen::Hotkeys {
            self.edits = ["double_tap", "home", "lock", "jump"]
                .into_iter()
                .map(|key| {
                    let key = format!("hotkeys.{key}");
                    let value = config.get(&key).unwrap_or_default();
                    (key, value)
                })
                .collect();
        } else if let Some(status) = &self.status {
            for peer in &status.peers {
                let settings = config.peer(&peer.id).cloned().unwrap_or_default();
                self.edits.push((
                    format!("peers.{}.allow_drive", peer.id),
                    settings.allow_drive.to_string(),
                ));
                let clipboard = match settings.clipboard {
                    a7n_ipc::config::ClipboardPolicy::Both => "both",
                    a7n_ipc::config::ClipboardPolicy::Send => "send",
                    a7n_ipc::config::ClipboardPolicy::Receive => "receive",
                    a7n_ipc::config::ClipboardPolicy::Off => "off",
                };
                self.edits
                    .push((format!("peers.{}.clipboard", peer.id), clipboard.into()));
                for source in [
                    Modifier::LeftCtrl,
                    Modifier::LeftShift,
                    Modifier::LeftAlt,
                    Modifier::LeftMeta,
                    Modifier::RightCtrl,
                    Modifier::RightShift,
                    Modifier::RightAlt,
                    Modifier::RightMeta,
                ] {
                    let target = settings
                        .modifier_remap
                        .get(&source)
                        .copied()
                        .unwrap_or(source);
                    let source = modifier_name(source);
                    self.edits.push((
                        format!("peers.{}.modifier_remap.{source}", peer.id),
                        modifier_name(target).into(),
                    ));
                }
            }
        }
    }
}

fn modifier_name(modifier: a7n_ipc::config::Modifier) -> &'static str {
    use a7n_ipc::config::Modifier;
    match modifier {
        Modifier::LeftCtrl => "left_ctrl",
        Modifier::LeftShift => "left_shift",
        Modifier::LeftAlt => "left_alt",
        Modifier::LeftMeta => "left_meta",
        Modifier::RightCtrl => "right_ctrl",
        Modifier::RightShift => "right_shift",
        Modifier::RightAlt => "right_alt",
        Modifier::RightMeta => "right_meta",
    }
}

fn parse_address(text: &str) -> Result<SocketAddr, ()> {
    text.trim()
        .parse()
        .or_else(|_| {
            text.trim()
                .parse::<IpAddr>()
                .map(|ip| SocketAddr::new(ip, DEFAULT_UDP_PORT))
        })
        .map_err(|_| ())
}
