// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::config::{MAX_PEER_SPEED, MIN_PEER_SPEED};
use a7n_ipc::{LinkProblem, PeerInfo};
use eframe::egui;

use crate::{App, theme};
use theme::{BadgeKind, ButtonKind, ButtonSize, icon};

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
const PEER_DETAILS_MIN_WIDTH: f32 = 640.0;
const PEER_LIST_WIDTH: f32 = 200.0;

impl App {
    pub(super) fn peers(&mut self, ui: &mut egui::Ui) {
        self.receive_directory(ui);
        ui.add_space(theme::size::SECTION_GAP);
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
            self.selected_peer = None;
            theme::empty_card(
                ui,
                icon::SHIELD_CHECK,
                "No paired machines yet.",
                "Pair a machine on the Machines page",
            );
            return;
        }
        if !peers.iter().any(|peer| Some(peer.id) == self.selected_peer) {
            self.selected_peer = Some(peers[0].id);
        }
        let wide = ui.available_width() >= PEER_LIST_WIDTH + PEER_DETAILS_MIN_WIDTH + 16.0;
        let layout = if wide {
            egui::Layout::left_to_right(egui::Align::Min)
        } else {
            egui::Layout::top_down(egui::Align::Min)
        };
        ui.with_layout(layout, |ui| {
            let list_width = if wide {
                PEER_LIST_WIDTH
            } else {
                ui.available_width()
            };
            ui.allocate_ui_with_layout(
                egui::vec2(list_width, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| self.peer_list(ui, &peers),
            );
            ui.add_space(8.0);
            ui.vertical(|ui| {
                if let Some(peer) = peers
                    .iter()
                    .find(|peer| Some(peer.id) == self.selected_peer)
                {
                    self.peer_settings(ui, peer);
                }
            });
        });
    }

    fn peer_list(&mut self, ui: &mut egui::Ui, peers: &[PeerInfo]) {
        theme::overline(ui, "Paired machines");
        for peer in peers {
            ui.push_id(peer.id, |ui| {
                if theme::nav_item(
                    ui,
                    icon::MONITOR,
                    &peer.name,
                    Some(peer.id) == self.selected_peer,
                )
                .clicked()
                {
                    self.selected_peer = Some(peer.id);
                }
                theme::badge(
                    ui,
                    if peer.connected {
                        BadgeKind::Online
                    } else {
                        BadgeKind::Offline
                    },
                    if peer.connected { "Online" } else { "Offline" },
                );
                peer_problem(ui, peer);
                let id = peer.id.to_string();
                ui.label(
                    egui::RichText::new(format!("{}…{}", &id[..8], &id[id.len() - 4..]))
                        .font(theme::font_mono(12.0))
                        .color(theme::Palette::of(ui.ctx()).text3),
                );
                ui.add_space(theme::size::ROW_GAP);
            });
        }
    }

    fn receive_directory(&mut self, ui: &mut egui::Ui) {
        let key = "files.receive_dir";
        let mut value = self.value(key, "");
        theme::card(ui, |ui| {
            theme::setting_row(
                ui,
                "Receive directory",
                "Files from all peers are saved here. Use an absolute path.",
                Some(key),
                |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), theme::size::CONTROL_H),
                        egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                        |ui| {
                            if self.editing_receive_dir {
                                if theme::text_input(
                                    ui,
                                    "Directory",
                                    &mut value,
                                    "Absolute path",
                                    true,
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
                                    "Done",
                                )
                                .clicked()
                                {
                                    self.editing_receive_dir = false;
                                }
                            } else {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&value).font(theme::font_mono(13.0)),
                                    )
                                    .truncate(),
                                );
                                if theme::button(
                                    ui,
                                    ButtonKind::Ghost,
                                    ButtonSize::Sm,
                                    None,
                                    "Change…",
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
        theme::overline(ui, &peer.name);
        peer_problem(ui, peer);
        theme::card(ui, |ui| {
            let key = format!("{prefix}.allow_drive");
            let mut allowed = self.value(&key, "true") == "true";
            theme::setting_row(
                ui,
                "Allow drive",
                "This machine may take over keyboard and mouse here.",
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
                "Drive this machine from here",
                "Off: edges, jump keys and switching never move input to this machine.",
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
                "Wake automatically",
                "Wake this machine when you switch to it while offline.",
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
                    "Clipboard",
                    "both",
                    "Direction clipboard contents are shared.",
                ),
                (
                    "files",
                    "Files",
                    "ask",
                    "Ask shows a confirmation before files are saved.",
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
                            ("both", "Both"),
                            ("send", "Send"),
                            ("receive", "Receive"),
                            ("ask", "Ask"),
                            ("off", "Off"),
                        ],
                    )
                    .changed()
                    {
                        self.edit(&key, selected, default);
                    }
                });
                if name == "clipboard" && selected == "ask" {
                    theme::small(ui, "Incoming copies wait for your confirmation.");
                }
            }
        });
        ui.add_space(theme::size::SECTION_GAP);
        theme::card(ui, |ui| self.peer_input(ui, &prefix));
        ui.add_space(theme::size::SECTION_GAP);
        theme::card(ui, |ui| self.modifier_remaps(ui, &prefix));
    }

    fn peer_input(&mut self, ui: &mut egui::Ui, prefix: &str) {
        ui.heading("Input from this machine");
        for (name, title, help) in [
            (
                "pointer_speed",
                "Pointer speed",
                "Multiplies your base pointer speed when this machine drives you.",
            ),
            (
                "scroll_speed",
                "Scroll speed",
                "Wheel and trackpad speed when this machine drives you.",
            ),
        ] {
            let key = format!("{prefix}.{name}");
            let mut percent = self.value(&key, "1.0").parse::<f64>().unwrap_or(1.0) * 100.0;
            theme::setting_row(ui, title, help, None, |ui| {
                if ui
                    .add(
                        egui::Slider::new(
                            &mut percent,
                            MIN_PEER_SPEED * 100.0..=MAX_PEER_SPEED * 100.0,
                        )
                        .suffix(" %"),
                    )
                    .changed()
                {
                    self.edit(&key, &(percent / 100.0).to_string(), "1.0");
                }
            });
        }
        let key = format!("{prefix}.reverse_scroll");
        let mut reverse = self.value(&key, "false") == "true";
        theme::setting_row(
            ui,
            "Reverse scrolling",
            "Reverse both scroll directions received from this machine.",
            None,
            |ui| {
                if ui.checkbox(&mut reverse, "Reverse scrolling").changed() {
                    self.edit(&key, if reverse { "true" } else { "false" }, "false");
                }
            },
        );
    }

    fn modifier_remaps(&mut self, ui: &mut egui::Ui, prefix: &str) {
        let mode_key = format!("{prefix}.modifier_mode");
        let value = self.value(&mode_key, "auto");
        let mut selected = value.as_str();
        theme::setting_row(
            ui,
            "Modifier remaps",
            "Auto swaps Cmd and Ctrl between a Mac and a PC.",
            Some("modifier_mode"),
            |ui| {
                if theme::segmented(
                    ui,
                    &mut selected,
                    &[("auto", "Auto"), ("off", "Off"), ("custom", "Custom")],
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
            "Modifier remaps",
            "Physical key on this keyboard → key sent to the peer. Same key = unchanged.",
            Some("modifier_remap"),
            |ui| {
                if theme::button(ui, ButtonKind::Ghost, ButtonSize::Sm, None, "Reset all").clicked()
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
        LinkProblem::Unreachable => "Unreachable: check both machines are on the same network and the firewall allows apeirogon.".to_owned(),
        LinkProblem::Refused => "Not accepting connections: is apeirogon running there?".to_owned(),
        LinkProblem::AuthFailed => "Certificate does not match: pair again.".to_owned(),
        LinkProblem::VersionMismatch { older: Some(name) } => format!("Different apeirogon versions: update {name}."),
        LinkProblem::VersionMismatch { older: None } => "Different apeirogon versions: update both machines.".to_owned(),
        LinkProblem::Silent => "Stopped responding: check the peer and its network connection.".to_owned(),
    };
    theme::small(ui, &message);
}
