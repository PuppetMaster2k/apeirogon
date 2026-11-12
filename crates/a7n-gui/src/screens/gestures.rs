// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui;
use rust_i18n::t;

use crate::{App, theme};
use theme::{ButtonKind, ButtonSize};

use super::chord_editor::chord_editor;

impl App {
    pub(super) fn gestures(&mut self, ui: &mut egui::Ui) {
        let count = ["swipe_up", "swipe_down", "swipe_left", "swipe_right"]
            .iter()
            .filter(|name| !self.value(&format!("gestures.{name}"), "").is_empty())
            .count();
        let summary = t!("%{count} of %{total} on", count = count, total = 4);
        theme::accordion(
            ui,
            "gestures",
            &t!("Trackpad gestures"),
            Some(&summary),
            false,
            false,
            |ui| {
                theme::small(
                    ui,
                    &t!("Mac trackpad swipes trigger these Windows shortcuts."),
                );
                for (index, (name, title)) in [
                    ("swipe_up", &t!("Swipe up")),
                    ("swipe_down", &t!("Swipe down")),
                    ("swipe_left", &t!("Swipe left")),
                    ("swipe_right", &t!("Swipe right")),
                ]
                .into_iter()
                .enumerate()
                {
                    ui.separator();
                    self.gesture_row(ui, index, name, title);
                }
            },
        );
    }

    fn gesture_row(&mut self, ui: &mut egui::Ui, index: usize, name: &str, title: &str) {
        let key = format!("gestures.{name}");
        theme::setting_row(ui, title, &t!("Clear to turn off."), Some(&key), |ui| {
            self.gesture_editor(ui, index, &key);
        });
    }

    fn gesture_editor(&mut self, ui: &mut egui::Ui, index: usize, key: &str) {
        let mut value = self.value(key, "");
        if value.is_empty() && !self.editing_gestures[index] {
            theme::small(ui, &t!("Off"));
        }
        let can_clear = !value.is_empty();
        let mut clear = false;
        if chord_editor(
            ui,
            &mut value,
            &mut self.editing_gestures[index],
            "ctrl+meta+left",
            |ui| {
                if can_clear {
                    clear =
                        theme::button(ui, ButtonKind::Ghost, ButtonSize::Sm, None, &t!("Clear"))
                            .clicked();
                }
            },
        ) {
            self.edit(key, &value, "");
        }
        if clear {
            self.edit(key, "", "");
        }
    }
}
