// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui;
use rust_i18n::t;

use crate::{App, theme};

use super::chord_editor::chord_editor;
use theme::{ButtonKind, ButtonSize};

impl App {
    pub(super) fn hotkeys(&mut self, ui: &mut egui::Ui) {
        theme::card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = theme::size::ROW_GAP;
            for (index, (name, title, help)) in [
                (
                    "double_tap",
                    &t!("Trigger key"),
                    &t!("Double-tap or hold this key to return to this machine. Holding it with another key does nothing."),
                ),
                ("home", &t!("Home"), &t!("Return keyboard and mouse to this machine.")),
                (
                    "lock",
                    &t!("Lock"),
                    &t!("Keep input on the current machine; press again to unlock."),
                ),
                (
                    "jump",
                    &t!("Jump"),
                    &t!("Hold with 1–9 to jump to a paired machine. Unpaired slots do nothing; other numbers stay the same."),
                ),
            ]
            .into_iter()
            .enumerate()
            {
                if index > 0 {
                    ui.separator();
                }
                self.hotkey_row(ui, index, name, title, help);
            }
            ui.separator();
            self.trigger_options(ui);
            ui.separator();
            self.shake_option(ui);
        });
    }

    fn hotkey_row(&mut self, ui: &mut egui::Ui, index: usize, name: &str, title: &str, help: &str) {
        let key = format!("hotkeys.{name}");
        let mut value = self.value(&key, "");
        theme::setting_row(ui, title, help, Some(&key), |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), theme::size::CONTROL_H),
                egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                |ui| {
                    let kind = match name {
                        "double_tap" => a7n_ipc::HotkeyKind::Trigger,
                        "jump" => a7n_ipc::HotkeyKind::Modifiers,
                        _ => a7n_ipc::HotkeyKind::Chord,
                    };
                    self.record_hotkey(ui, &key, kind);
                    let placeholder = match name {
                        "home" => "scroll_lock,ctrl+alt+shift+home",
                        "jump" => "ctrl+alt+shift",
                        "lock" => "ctrl+alt+shift+l",
                        _ => "right_alt",
                    };
                    if chord_editor(
                        ui,
                        &mut value,
                        &mut self.editing_hotkeys[index],
                        placeholder,
                    ) {
                        self.edit(&key, &value, "");
                    }
                },
            );
        });
        if name == "jump" {
            theme::small(
                ui,
                &t!(
                    "Windows' Alt+Shift input-language hotkey also fires on this chord; turn it off under Settings > Time & language > Typing > Advanced keyboard settings > Input language hot keys, or pick other jump keys.",
                ),
            );
        }
    }

    pub(super) fn record_hotkey(
        &mut self,
        ui: &mut egui::Ui,
        key: &str,
        kind: a7n_ipc::HotkeyKind,
    ) {
        ui.add_enabled_ui(self.recording_hotkey.is_none(), |ui| {
            let label = &if self.recording_hotkey.as_deref() == Some(key) {
                t!("Press keys…")
            } else {
                t!("Record")
            };
            if theme::button(ui, ButtonKind::Ghost, ButtonSize::Sm, None, label).clicked() {
                self.recording_hotkey = Some(key.to_owned());
                self.send(crate::Command::Request(a7n_ipc::Request::RecordHotkey {
                    kind,
                }));
            }
        });
    }

    fn shake_option(&mut self, ui: &mut egui::Ui) {
        let key = "hotkeys.shake";
        let mut value = self.value(key, "");
        theme::setting_row(
            ui,
            &t!("Shake to return"),
            &t!(
                "Shake the mouse left and right this many times to return to this machine. Enter zero to turn it off.",
            ),
            Some(key),
            |ui| {
                if theme::text_input(ui, &t!("Number of reversals"), &mut value, "", true).changed()
                {
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
            &t!("Trigger mode"),
            &t!("How the trigger returns input home."),
            Some(key),
            |ui| {
                egui::ComboBox::from_id_salt(key)
                    .selected_text(match mode.as_str() {
                        "hold" => t!("Hold"),
                        "both" => t!("Both"),
                        _ => t!("Double tap"),
                    })
                    .show_ui(ui, |ui| {
                        for (value, label) in [
                            ("double_tap", t!("Double tap")),
                            ("hold", t!("Hold")),
                            ("both", t!("Both")),
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
            &t!("Hold duration (ms)"),
            &t!("300–3000 ms; default 800 ms."),
            Some(key),
            |ui| {
                if theme::text_input(
                    ui,
                    &t!("Hold duration in milliseconds"),
                    &mut value,
                    "800",
                    true,
                )
                .changed()
                {
                    self.edit(key, &value, "");
                }
            },
        );
    }
}
