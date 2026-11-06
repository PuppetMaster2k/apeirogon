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
                    "Trigger key",
                    "Double-tap or hold this key to return to this machine. Holding it with another key does nothing.",
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
                            if index == 0 {
                                ui.add_enabled_ui(!self.recording_trigger, |ui| {
                                    if theme::button(ui, ButtonKind::Ghost, ButtonSize::Sm, None,
                                        if self.recording_trigger { "Press a key…" } else { "Record" })
                                        .clicked()
                                    {
                                        self.recording_trigger = true;
                                        self.send(crate::Command::Request(a7n_ipc::Request::RecordTriggerKey));
                                    }
                                });
                            }
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
            ui.separator();
            self.trigger_options(ui);
            ui.separator();
            self.shake_option(ui);
        });
    }

    fn shake_option(&mut self, ui: &mut egui::Ui) {
        let key = "hotkeys.shake";
        let mut value = self.value(key, "");
        theme::setting_row(
            ui,
            "Shake to return",
            "Shake the mouse left and right this many times to return to this machine. 0 turns it off.",
            Some(key),
            |ui| {
                if theme::text_input(ui, "Number of reversals", &mut value, "4", true).changed() {
                    self.edit(key, &value, "");
                }
            },
        );
    }

    fn trigger_options(&mut self, ui: &mut egui::Ui) {
        let key = "hotkeys.trigger_mode";
        let mut mode = self.value(key, "double_tap");
        theme::setting_row(
            ui,
            "Trigger mode",
            "How the trigger returns input home.",
            Some(key),
            |ui| {
                egui::ComboBox::from_id_salt(key)
                    .selected_text(match mode.as_str() {
                        "hold" => "Hold",
                        "both" => "Both",
                        _ => "Double tap",
                    })
                    .show_ui(ui, |ui| {
                        for (value, label) in [
                            ("double_tap", "Double tap"),
                            ("hold", "Hold"),
                            ("both", "Both"),
                        ] {
                            if ui
                                .selectable_value(&mut mode, value.into(), label)
                                .changed()
                            {
                                self.edit(key, &mode, "double_tap");
                            }
                        }
                    });
            },
        );
        if mode == "double_tap" {
            return;
        }
        let key = "hotkeys.hold_ms";
        let mut value = self.value(key, "");
        theme::setting_row(
            ui,
            "Hold duration (ms)",
            "300–3000 ms; default 800 ms.",
            Some(key),
            |ui| {
                if theme::text_input(ui, "Hold duration in milliseconds", &mut value, "800", true)
                    .changed()
                {
                    self.edit(key, &value, "");
                }
            },
        );
    }
}
