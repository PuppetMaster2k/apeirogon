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
        let body_visuals = ui.visuals().clone();
        ui.visuals_mut().collapsing_header_frame = true;
        let p = Palette::of(ui.ctx());
        ui.visuals_mut().widgets.inactive.expansion = 0.0;
        ui.visuals_mut().widgets.hovered.expansion = 0.0;
        ui.visuals_mut().widgets.inactive.bg_stroke = egui::Stroke::NONE;
        ui.visuals_mut().widgets.hovered.bg_stroke = egui::Stroke::NONE;
        ui.visuals_mut().widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
        ui.visuals_mut().widgets.hovered.weak_bg_fill = p.hover;
        ui.visuals_mut().widgets.active.expansion = 0.0;
        ui.visuals_mut().widgets.active.bg_stroke = egui::Stroke::new(2.0, p.accent);
        egui::CollapsingHeader::new(title)
            .id_salt(id)
            .default_open(default_open)
            .show(ui, |ui| {
                *ui.visuals_mut() = body_visuals;
                add(ui)
            })
            .body_returned
    })
    .inner
}
