// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui;

use crate::theme::{self, ButtonKind, ButtonSize};

pub(super) fn chord_editor(
    ui: &mut egui::Ui,
    value: &mut String,
    editing: &mut bool,
    placeholder: &str,
) -> bool {
    if *editing {
        ui.set_max_width(320.0);
        let changed = theme::text_input(ui, "Shortcut", value, placeholder, true).changed();
        if theme::button(ui, ButtonKind::Ghost, ButtonSize::Sm, None, "Done").clicked() {
            *editing = false;
        }
        return changed;
    }
    for (index, chord) in value
        .split(',')
        .filter(|chord| !chord.is_empty())
        .enumerate()
    {
        if index > 0 {
            theme::small(ui, "or");
        }
        theme::chord(ui, chord.trim());
    }
    if theme::button(ui, ButtonKind::Ghost, ButtonSize::Sm, None, "Edit").clicked() {
        *editing = true;
    }
    false
}
