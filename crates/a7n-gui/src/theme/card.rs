// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui;
use eframe::egui::{Align2, CornerRadius, Frame, Margin, Sense, Stroke, Ui, Vec2};

use super::{Palette, font_icon, size, small};

/// Bordered surface card, 20 px padding.
pub(crate) fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = Palette::of(ui.ctx());
    Frame::new()
        .fill(ui.visuals().widgets.noninteractive.bg_fill)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(size::R_CARD))
        .inner_margin(Margin::same(20))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

pub(crate) fn icon_tile(ui: &mut Ui, icon: &str) {
    let p = Palette::of(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
    ui.painter().rect_filled(rect, size::R_CARD, p.hover);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        icon,
        font_icon(size::ICON_LG),
        p.text2,
    );
}

/// Empty sections retain a visible boundary and an explanation.
pub(crate) fn empty_card(ui: &mut Ui, icon: &str, sentence: &str, hint: &str) {
    let p = Palette::of(ui.ctx());
    let response = Frame::new()
        .inner_margin(20)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                icon_tile(ui, icon);
                ui.vertical(|ui| {
                    ui.label(sentence);
                    small(ui, hint);
                });
            });
        })
        .response;
    let rect = response.rect;
    let points = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    ui.painter().extend(egui::Shape::dashed_line(
        &points,
        Stroke::new(1.0, p.border_strong),
        4.0,
        3.0,
    ));
}
