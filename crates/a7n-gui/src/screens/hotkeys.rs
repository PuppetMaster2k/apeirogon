// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui;

use crate::{App, theme};
use theme::{ButtonKind, ButtonSize};

impl App {
    pub(super) fn hotkeys(&mut self, ui: &mut egui::Ui) {
        theme::card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = theme::size::ROW_GAP;
            for (index, (name, title, help)) in [
                (
                    "double_tap",
                    "Double tap",
                    "Tap twice to return to this machine.",
                ),
                ("home", "Home", "Return keyboard and mouse to this machine."),
                (
                    "lock",
                    "Lock",
                    "Keep input on the current machine; press again to unlock.",
                ),
                ("jump", "Jump", "Hold with 1–9 to jump to a machine."),
            ]
            .into_iter()
            .enumerate()
            {
                if index > 0 {
                    ui.separator();
                }
                let key = format!("hotkeys.{name}");
                let mut value = self.value(&key, "");
                theme::setting_row(ui, title, help, Some(&key), |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), theme::size::CONTROL_H),
                        egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                        |ui| {
                            if self.editing_hotkeys[index] {
                                ui.set_max_width(320.0);
                                if theme::text_input(
                                    ui,
                                    "Shortcut",
                                    &mut value,
                                    if name == "home" {
                                        "scroll_lock,ctrl+alt+shift+home"
                                    } else {
                                        "right_alt"
                                    },
                                    true,
                                )
                                .changed()
                                {
                                    self.edit(&key, &value, "");
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
                                    self.editing_hotkeys[index] = false;
                                }
                            } else {
                                for (chord_index, chord) in value.split(',').enumerate() {
                                    if chord_index > 0 {
                                        theme::small(ui, "or");
                                    }
                                    theme::chord(ui, chord.trim());
                                }
                                if theme::button(
                                    ui,
                                    ButtonKind::Ghost,
                                    ButtonSize::Sm,
                                    None,
                                    "Edit",
                                )
                                .clicked()
                                {
                                    self.editing_hotkeys[index] = true;
                                }
                            }
                        },
                    );
                });
            }
        });
    }
}
