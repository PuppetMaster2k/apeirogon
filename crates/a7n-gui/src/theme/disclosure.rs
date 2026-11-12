// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui::{self, Ui};

use super::Palette;

pub(crate) fn disclosure<R>(
    ui: &mut Ui,
    id: impl egui::AsIdSalt,
    title: &str,
    default_open: bool,
    add: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    ui.scope(|ui| {
        ui.spacing_mut().indent = 24.0;
        ui.visuals_mut().indent_has_left_vline = true;
        ui.visuals_mut().widgets.noninteractive.bg_stroke.color = Palette::of(ui.ctx()).border;
        egui::CollapsingHeader::new(title)
            .id_salt(id)
            .default_open(default_open)
            .show(ui, add)
            .body_returned
    })
    .inner
}
