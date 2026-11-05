// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui;
use eframe::egui::{Color32, CornerRadius, Stroke, StrokeKind, Ui};

use super::Palette;

pub(super) fn focus_ring(ui: &Ui, rect: egui::Rect, radius: u8) {
    let p = Palette::of(ui.ctx());
    ui.painter().rect_stroke(
        rect.expand(1.5),
        CornerRadius::same(radius + 2),
        Stroke::new(3.0, p.focus),
        StrokeKind::Outside,
    );
}

pub(super) fn fade(c: Color32, enabled: bool) -> Color32 {
    if enabled { c } else { c.gamma_multiply(0.45) }
}
