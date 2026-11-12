// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui;
use rust_i18n::t;

use crate::theme::{self, ButtonKind, ButtonSize};

pub(super) fn chord_editor(
    ui: &mut egui::Ui,
    value: &mut String,
    editing: &mut bool,
    placeholder: &str,
    actions: impl FnOnce(&mut egui::Ui),
) -> bool {
    if *editing {
        let p = theme::Palette::of(ui.ctx());
        ui.label(
            egui::RichText::new(t!("Shortcut"))
                .font(theme::font_medium(13.0))
                .color(p.text),
        );
    }
    ui.horizontal_wrapped(|ui| {
        ui.set_min_height(theme::size::CONTROL_H);
        let changed = if *editing {
            let width = ui.available_width().min(240.0);
            let changed = ui
                .allocate_ui_with_layout(
                    egui::vec2(width, theme::size::CONTROL_H),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| theme::text_field(ui, value, placeholder, true, false).changed(),
                )
                .inner;
            if theme::button(ui, ButtonKind::Ghost, ButtonSize::Sm, None, &t!("Done")).clicked() {
                *editing = false;
            }
            changed
        } else {
            for (index, chord) in value
                .split(',')
                .filter(|chord| !chord.is_empty())
                .enumerate()
            {
                if index > 0 {
                    theme::small(ui, &t!("or"));
                }
                theme::chord(ui, chord.trim());
            }
            if theme::button(ui, ButtonKind::Ghost, ButtonSize::Sm, None, &t!("Edit")).clicked() {
                *editing = true;
            }
            false
        };
        actions(ui);
        changed
    })
    .inner
}

pub(super) fn chord_summary(value: &str) -> String {
    if value.is_empty() {
        return t!("Off").into_owned();
    }
    value
        .split(',')
        .take(3)
        .map(|chord| {
            chord
                .split('+')
                .map(theme::key_name)
                .collect::<Vec<_>>()
                .join("+")
        })
        .collect::<Vec<_>>()
        .join(" · ")
}
