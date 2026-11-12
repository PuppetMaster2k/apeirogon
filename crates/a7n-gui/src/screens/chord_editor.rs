// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui;
use rust_i18n::t;

use crate::theme::{self, ButtonKind, ButtonSize};

pub(super) fn chord_editor(
    ui: &mut egui::Ui,
    value: &mut String,
    editing: &mut bool,
    placeholder: &str,
) -> bool {
    if *editing {
        ui.set_max_width(320.0);
        let changed = theme::text_input(ui, &t!("Shortcut"), value, placeholder, true).changed();
        if theme::button(ui, ButtonKind::Ghost, ButtonSize::Sm, None, &t!("Done")).clicked() {
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
            theme::small(ui, &t!("or"));
        }
        theme::chord(ui, chord.trim());
    }
    if theme::button(ui, ButtonKind::Ghost, ButtonSize::Sm, None, &t!("Edit")).clicked() {
        *editing = true;
    }
    false
}
