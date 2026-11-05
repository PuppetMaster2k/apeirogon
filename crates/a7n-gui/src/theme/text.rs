// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui::{RichText, Ui};

use super::{Palette, font_icon, icon, size};

/// Quiet help text shared by screens.
pub(crate) fn small(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .small()
            .color(Palette::of(ui.ctx()).text3),
    );
}

pub(crate) fn error(ui: &mut Ui, text: &str) {
    let p = Palette::of(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(icon::WARNING_CIRCLE)
                .font(font_icon(size::ICON))
                .color(p.danger),
        );
        ui.label(RichText::new(text).small().color(p.danger));
    });
}
