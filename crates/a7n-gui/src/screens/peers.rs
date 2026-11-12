// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::config::{MAX_PEER_SPEED, MIN_PEER_SPEED};
use a7n_ipc::{LinkProblem, PeerInfo};
use eframe::egui;
use rust_i18n::t;

use crate::{App, theme};
use theme::{BadgeKind, ButtonKind, ButtonSize, IconButtonKind, icon};

const MODIFIERS: [&str; 8] = [
    "left_ctrl",
    "right_ctrl",
    "left_shift",
    "right_shift",
    "left_alt",
    "right_alt",
    "left_meta",
    "right_meta",
];
impl App {
    pub(super) fn peers(&mut self, ui: &mut egui::Ui) {
        theme::overline(ui, &t!("Receive directory"));
        self.receive_directory(ui);
        ui.add_space(theme::size::SECTION_GAP);
        theme::overline(ui, &t!("Paired machines"));
        let Some(status) = &self.status else {
            return;
        };
        let peers: Vec<_> = status
            .peers
            .iter()
            .filter(|peer| peer.id != status.machine)
            .cloned()
            .collect();
        if peers.is_empty() {
            theme::empty_card(
                ui,
                icon::SHIELD_CHECK,
                &t!("No paired machines yet."),
                &t!("Pair a machine on the Machines page"),
            );
        }
        for peer in peers {
            ui.push_id(peer.id, |ui| self.peer_card(ui, &peer));
        }
    }

    fn peer_card(&mut self, ui: &mut egui::Ui, peer: &PeerInfo) {
        theme::card(ui, |ui| {
            let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                ui.make_persistent_id("peer_settings"),
                false,
            );
            let prefix = format!("peers.{}", peer.id);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (chevron, tooltip) = if state.is_open() {
                        (icon::CARET_DOWN, t!("Collapse peer settings"))
                    } else {
                        (icon::CARET_RIGHT, t!("Expand peer settings"))
                    };
                    if theme::icon_button(ui, IconButtonKind::Ghost, chevron, &tooltip, false)
                        .clicked()
                    {
                        state.toggle(ui);
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.vertical(|ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    egui::RichText::new(&peer.name).font(theme::font_medium(14.0)),
                                );
                                theme::badge(
                                    ui,
                                    if peer.connected {
                                        BadgeKind::Online
                                    } else {
                                        BadgeKind::Offline
                                    },
                                    &if peer.connected {
                                        t!("Online")
                                    } else {
                                        t!("Offline")
                                    },
                                );
                            });
                            ui.label(
                                egui::RichText::new(&prefix)
                                    .font(theme::font_mono(12.0))
                                    .color(theme::Palette::of(ui.ctx()).text3),
                            );
                        });
                    });
                });
            });
            if !state.is_open() {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(self.peer_summary(&prefix))
                            .size(12.0)
                            .color(theme::Palette::of(ui.ctx()).text3),
                    )
                    .truncate(),
                );
            }
            peer_problem(ui, peer);
            state.show_body_unindented(ui, |ui| self.peer_settings(ui, peer));
        });
    }

    fn peer_summary(&self, prefix: &str) -> String {
        let drive = if self.value(&format!("{prefix}.allow_drive"), "true") == "true" {
            t!("Drive allowed")
        } else {
            t!("Drive denied")
        };
        let clipboard = match self.value(&format!("{prefix}.clipboard"), "both").as_str() {
            "send" => t!("Clipboard send"),
            "receive" => t!("Clipboard receive"),
            "ask" => t!("Clipboard ask"),
            "off" => t!("Clipboard off"),
            _ => t!("Clipboard both"),
        };
        let remaps = match self
            .value(&format!("{prefix}.modifier_mode"), "auto")
            .as_str()
        {
            "off" => t!("No remaps"),
            "custom" => {
                let count = MODIFIERS
                    .iter()
                    .filter(|source| {
                        self.value(&format!("{prefix}.modifier_remap.{source}"), source) != **source
                    })
                    .count();
                if count == 0 {
                    t!("No remaps")
                } else {
                    t!("%{count} remaps", count = count)
                }
            }
            _ => t!("Auto remaps"),
        };
        format!("{drive} · {clipboard} · {remaps}")
    }

    fn receive_directory(&mut self, ui: &mut egui::Ui) {
        let key = "files.receive_dir";
        let mut value = self.value(key, "");
        theme::card(ui, |ui| {
            theme::setting_row(
                ui,
                &t!("Receive directory"),
                &t!("Files from all peers are saved here. Use an absolute path."),
                Some(key),
                |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), theme::size::CONTROL_H),
                        egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                        |ui| {
                            if self.editing_receive_dir {
                                if theme::text_input(
                                    ui,
                                    &t!("Directory"),
                                    &mut value,
                                    &t!("Absolute path"),
                                    false,
                                )
                                .changed()
                                {
                                    self.edit(key, &value, "");
                                }
                                if theme::button(
                                    ui,
                                    ButtonKind::Ghost,
                                    ButtonSize::Sm,
                                    None,
                                    &t!("Done"),
                                )
                                .clicked()
                                {
                                    self.editing_receive_dir = false;
                                }
                            } else {
                                ui.add(egui::Label::new(egui::RichText::new(&value)).truncate());
                                if theme::button(
                                    ui,
                                    ButtonKind::Ghost,
                                    ButtonSize::Sm,
                                    None,
                                    &t!("Change…"),
                                )
                                .clicked()
                                {
                                    self.editing_receive_dir = true;
                                }
                            }
                        },
                    );
                },
            );
        });
    }

    fn peer_settings(&mut self, ui: &mut egui::Ui, peer: &PeerInfo) {
        let prefix = format!("peers.{}", peer.id);
        theme::overline(ui, &t!("Peers & policies"));
        ui.scope(|ui| {
            let key = format!("{prefix}.allow_drive");
            let mut allowed = self.value(&key, "true") == "true";
            theme::setting_row(
                ui,
                &t!("Allow drive"),
                &t!("This machine may take over keyboard and mouse here."),
                Some("allow_drive"),
                |ui| {
                    if theme::toggle(ui, &mut allowed).changed() {
                        self.edit(&key, if allowed { "true" } else { "false" }, "true");
                    }
                },
            );
            ui.separator();
            let key = format!("{prefix}.drive_peer");
            let mut drive_peer = self.value(&key, "true") == "true";
            theme::setting_row(
                ui,
                &t!("Drive this machine from here"),
                &t!("Off: edges, jump keys and switching never move input to this machine."),
                Some("drive_peer"),
                |ui| {
                    if theme::toggle(ui, &mut drive_peer).changed() {
                        self.edit(&key, if drive_peer { "true" } else { "false" }, "true");
                    }
                },
            );
            ui.separator();
            let key = format!("{prefix}.wake");
            let mut wake = self.value(&key, "true") == "true";
            theme::setting_row(
                ui,
                &t!("Wake automatically"),
                &t!("Wake this machine when you switch to it while offline."),
                Some("wake"),
                |ui| {
                    if theme::toggle(ui, &mut wake).changed() {
                        self.edit(&key, if wake { "true" } else { "false" }, "true");
                    }
                },
            );
            for (name, title, default, help) in [
                (
                    "clipboard",
                    &t!("Clipboard"),
                    "both",
                    &t!("Direction clipboard contents are shared."),
                ),
                (
                    "files",
                    &t!("Files"),
                    "ask",
                    &t!("Ask shows a confirmation before files are saved."),
                ),
            ] {
                ui.separator();
                let key = format!("{prefix}.{name}");
                let value = self.value(&key, default);
                let mut selected = value.as_str();
                theme::setting_row(ui, title, help, Some(name), |ui| {
                    if theme::segmented(
                        ui,
                        &mut selected,
                        &[
                            ("both", &t!("Both")),
                            ("send", &t!("Send")),
                            ("receive", &t!("Receive")),
                            ("ask", &t!("Ask")),
                            ("off", &t!("Off")),
                        ],
                    )
                    .changed()
                    {
                        self.edit(&key, selected, default);
                    }
                });
                if name == "clipboard" && selected == "ask" {
                    theme::small(ui, &t!("Incoming copies wait for your confirmation."));
                }
            }
        });
        ui.add_space(theme::size::SECTION_GAP);
        theme::overline(ui, &t!("Input from this machine"));
        self.peer_input(ui, &prefix);
        ui.add_space(theme::size::SECTION_GAP);
        theme::overline(ui, &t!("Modifier remaps"));
        self.modifier_remaps(ui, &prefix);
    }

    fn peer_input(&mut self, ui: &mut egui::Ui, prefix: &str) {
        for (name, title, help) in [
            (
                "pointer_speed",
                &t!("Pointer speed"),
                &t!("Multiplies your base pointer speed when this machine drives you."),
            ),
            (
                "scroll_speed",
                &t!("Scroll speed"),
                &t!("Wheel and trackpad speed when this machine drives you."),
            ),
        ] {
            let key = format!("{prefix}.{name}");
            let mut speed = self.value(&key, "1.0").parse::<f64>().unwrap_or(1.0);
            if theme::slider(
                ui,
                title,
                &mut speed,
                MIN_PEER_SPEED..=MAX_PEER_SPEED,
                None,
                |value| format!("{value:.2}×"),
            )
            .changed()
            {
                self.edit(&key, &speed.to_string(), "1.0");
            }
            theme::small(ui, help);
        }
        theme::disclosure(
            ui,
            "advanced_input",
            &t!("Advanced settings"),
            false,
            |ui| {
                let key = format!("{prefix}.reverse_scroll");
                let mut reverse = self.value(&key, "false") == "true";
                theme::setting_row(
                    ui,
                    &t!("Reverse scrolling"),
                    &t!("Reverse both scroll directions received from this machine."),
                    None,
                    |ui| {
                        if theme::toggle(ui, &mut reverse).changed() {
                            self.edit(&key, if reverse { "true" } else { "false" }, "false");
                        }
                    },
                );
            },
        );
    }

    fn modifier_remaps(&mut self, ui: &mut egui::Ui, prefix: &str) {
        let mode_key = format!("{prefix}.modifier_mode");
        let value = self.value(&mode_key, "auto");
        let mut selected = value.as_str();
        theme::setting_row(
            ui,
            &t!("Modifier remaps"),
            &t!("Auto swaps Cmd and Ctrl between a Mac and a PC."),
            Some("modifier_mode"),
            |ui| {
                if theme::segmented(
                    ui,
                    &mut selected,
                    &[
                        ("auto", &t!("Auto")),
                        ("off", &t!("Off")),
                        ("custom", &t!("Custom")),
                    ],
                )
                .changed()
                {
                    self.edit(&mode_key, selected, "auto");
                }
            },
        );
        if selected != "custom" {
            return;
        }
        ui.separator();
        let key = format!("{prefix}.modifier_remap");
        theme::setting_row(
            ui,
            &t!("Modifier remaps"),
            &t!("Physical key on this keyboard → key sent to the peer. Same key = unchanged."),
            Some("modifier_remap"),
            |ui| {
                if theme::button(
                    ui,
                    ButtonKind::Ghost,
                    ButtonSize::Sm,
                    None,
                    &t!("Reset all"),
                )
                .clicked()
                {
                    for source in MODIFIERS {
                        self.edit(&format!("{key}.{source}"), source, source);
                    }
                }
            },
        );
        let columns = if ui.available_width() >= 600.0 { 2 } else { 1 };
        egui::Grid::new((prefix, "remaps"))
            .spacing(egui::vec2(8.0, theme::size::ROW_GAP))
            .show(ui, |ui| {
                for (index, source) in MODIFIERS.into_iter().enumerate() {
                    let key = format!("{prefix}.modifier_remap.{source}");
                    let mut value = self.value(&key, source);
                    theme::keycap(ui, source);
                    ui.label(
                        egui::RichText::new(icon::ARROW_RIGHT)
                            .font(theme::font_icon(theme::size::ICON))
                            .color(theme::Palette::of(ui.ctx()).text3),
                    );
                    ui.scope(|ui| {
                        let p = theme::Palette::of(ui.ctx());
                        if value != source {
                            ui.visuals_mut().widgets.inactive.bg_stroke =
                                egui::Stroke::new(1.0, p.accent);
                            ui.visuals_mut().widgets.inactive.fg_stroke.color = p.accent_text;
                            ui.visuals_mut().override_text_color = Some(p.accent_text);
                        }
                        egui::ComboBox::from_id_salt(&key)
                            .width(140.0)
                            .selected_text(
                                egui::RichText::new(theme::key_name(&value))
                                    .font(theme::font_mono(13.0)),
                            )
                            .show_ui(ui, |ui| {
                                for target in MODIFIERS {
                                    if ui
                                        .selectable_value(
                                            &mut value,
                                            target.to_owned(),
                                            egui::RichText::new(theme::key_name(target))
                                                .font(theme::font_mono(13.0)),
                                        )
                                        .changed()
                                    {
                                        self.edit(&key, &value, source);
                                    }
                                }
                            });
                    });
                    if (index + 1) % columns == 0 {
                        ui.end_row();
                    }
                }
            });
    }
}

pub(super) fn peer_problem(ui: &mut egui::Ui, peer: &PeerInfo) {
    if peer.connected {
        return;
    }
    let Some(problem) = &peer.problem else {
        return;
    };
    let message = match problem {
        LinkProblem::Unreachable => t!(
            "Unreachable: check both machines are on the same network and the firewall allows apeirogon."
        ),
        LinkProblem::Refused => t!("Not accepting connections: is apeirogon running there?"),
        LinkProblem::AuthFailed => t!("Certificate does not match: pair again."),
        LinkProblem::VersionMismatch { older: Some(name) } => {
            t!("Different apeirogon versions: update %{name}.", name = name)
        }
        LinkProblem::VersionMismatch { older: None } => {
            t!("Different apeirogon versions: update both machines.")
        }
        LinkProblem::Silent => t!("Stopped responding: check the peer and its network connection."),
    };
    theme::small(ui, &message);
}
