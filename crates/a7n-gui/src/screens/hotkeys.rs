// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui;
use rust_i18n::t;

use crate::{App, theme};

use super::chord_editor::chord_editor;
use theme::{ButtonKind, ButtonSize};

impl App {
    pub(super) fn hotkeys(&mut self, ui: &mut egui::Ui) {
        ui.scope(|ui| {
            for (index, (name, title, help)) in [
                (
                    "double_tap",
                    &t!("Trigger key"),
                    &t!("Tap twice or hold alone to return here."),
                ),
                ("home", &t!("Home"), &t!("Return input here.")),
                ("lock", &t!("Lock"), &t!("Press again to unlock.")),
                (
                    "jump",
                    &t!("Jump"),
                    &t!("Hold with 1–9 to switch; unpaired slots do nothing."),
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let value = self.value(&format!("hotkeys.{name}"), "");
                let summary = super::chord_editor::chord_summary(&value);
                let summary = if name == "double_tap" && !value.is_empty() {
                    let mode = match self.value("hotkeys.trigger_mode", "double_tap").as_str() {
                        "hold" => t!("Hold"),
                        "both" => t!("Both"),
                        _ => t!("Double tap"),
                    };
                    format!("{mode} · {summary}")
                } else {
                    summary
                };
                theme::accordion(ui, name, title, Some(&summary), false, false, |ui| {
                    self.hotkey_row(ui, index, name, title, help);
                    if name == "double_tap" {
                        self.trigger_options(ui);
                    }
                });
            }
            let count = self.value("hotkeys.shake", "6");
            let summary = if count == "0" {
                t!("Off").into_owned()
            } else {
                format!("{} · {count}", t!("On"))
            };
            theme::accordion(
                ui,
                "shake",
                &t!("Shake to return"),
                Some(&summary),
                false,
                false,
                |ui| self.shake_option(ui),
            );
        });
    }

    fn hotkey_row(&mut self, ui: &mut egui::Ui, index: usize, name: &str, title: &str, help: &str) {
        let key = format!("hotkeys.{name}");
        let mut value = self.value(&key, "");
        theme::setting_row(ui, title, help, Some(&key), |ui| {
            let kind = match name {
                "double_tap" => a7n_ipc::HotkeyKind::Trigger,
                "jump" => a7n_ipc::HotkeyKind::Modifiers,
                _ => a7n_ipc::HotkeyKind::Chord,
            };
            let placeholder = match name {
                "home" => "scroll_lock,ctrl+alt+shift+home",
                "jump" => "ctrl+alt+shift",
                "lock" => "ctrl+alt+shift+l",
                _ => "right_alt",
            };
            let mut editing = self.editing_hotkeys[index];
            if chord_editor(ui, &mut value, &mut editing, placeholder, |ui| {
                self.record_hotkey(ui, &key, kind);
            }) {
                self.edit(&key, &value, "");
            }
            self.editing_hotkeys[index] = editing;
        });
        if name == "jump" {
            theme::small(
                ui,
                &t!(
                    "Alt+Shift also switches Windows' input language; disable it in keyboard settings or choose another chord.",
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
        let mut value = self.value(key, "6").parse().unwrap_or(6);
        if theme::stepped_slider(
            ui,
            &t!("Shake to return"),
            &mut value,
            &[
                (0, "0"),
                (3, "3"),
                (4, "4"),
                (5, "5"),
                (6, "6"),
                (7, "7"),
                (8, "8"),
                (9, "9"),
                (10, "10"),
            ],
        )
        .changed()
        {
            self.edit(key, &value.to_string(), "6");
        }
        theme::small(
            ui,
            &t!("Shake left and right to return; zero turns it off."),
        );
    }

    fn trigger_options(&mut self, ui: &mut egui::Ui) {
        let key = "hotkeys.trigger_mode";
        let mut mode = self.value(key, "double_tap");
        theme::setting_row(
            ui,
            &t!("Trigger mode"),
            &t!("Tap twice, hold, or both."),
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
