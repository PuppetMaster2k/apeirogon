// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::PeerInfo;
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

impl App {
    pub(super) fn peers(&mut self, ui: &mut egui::Ui) {
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
        let list_width = 200.0;
        egui::ScrollArea::horizontal()
            .id_salt("peer_details")
            .show(ui, |ui| {
                ui.set_width(
                    ui.available_width()
                        .max(list_width + PEER_DETAILS_MIN_WIDTH + 16.0),
                );
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(list_width, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            theme::overline(ui, "Paired machines");
                            for peer in &peers {
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
                                    let id = peer.id.to_string();
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{}…{}",
                                            &id[..8],
                                            &id[id.len() - 4..]
                                        ))
                                        .font(theme::font_mono(12.0))
                                        .color(theme::Palette::of(ui.ctx()).text3),
                                    );
                                    ui.add_space(theme::size::ROW_GAP);
                                });
                            }
                        },
                    );
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width().max(PEER_DETAILS_MIN_WIDTH));
                        if let Some(peer) = peers
                            .iter()
                            .find(|peer| Some(peer.id) == self.selected_peer)
                        {
                            self.peer_settings(ui, peer);
                        }
                    });
                });
            });
    }

    fn peer_settings(&mut self, ui: &mut egui::Ui, peer: &PeerInfo) {
        let prefix = format!("peers.{}", peer.id);
        theme::overline(ui, &peer.name);
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
            let key = format!("{prefix}.clipboard");
            let value = self.value(&key, "both");
            let mut selected = value.as_str();
            theme::setting_row(
                ui,
                "Clipboard",
                "Direction clipboard contents are shared.",
                Some("clipboard"),
                |ui| {
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
                        self.edit(&key, selected, "both");
                    }
                },
            );
            if selected == "ask" {
                theme::small(ui, "Incoming copies wait for your confirmation.");
            }
        });
        ui.add_space(theme::size::SECTION_GAP);
        theme::card(ui, |ui| self.modifier_remaps(ui, &prefix));
    }

    fn modifier_remaps(&mut self, ui: &mut egui::Ui, prefix: &str) {
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
