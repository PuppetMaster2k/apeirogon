// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui::{Align, Layout, RichText, Ui, vec2};

use super::page_header::STACK_WIDTH;
use super::{Palette, font_medium, font_mono, size};

/// One setting inside a card: title + help (+ config key) on the left, control on the right.
pub(crate) fn setting_row(
    ui: &mut Ui,
    title: &str,
    help: &str,
    config_key: Option<&str>,
    control: impl FnOnce(&mut Ui),
) {
    let p = Palette::of(ui.ctx());
    let label = |ui: &mut Ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(RichText::new(title).font(font_medium(14.0)).color(p.text));
        ui.label(RichText::new(help).small().color(p.text3));
        if let Some(key) = config_key {
            ui.label(RichText::new(key).font(font_mono(12.0)).color(p.text3));
        }
    };
    if ui.available_width() < STACK_WIDTH {
        ui.vertical(label);
        ui.vertical(control);
        return;
    }
    let label_width = ui.available_width() * 0.48;
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(vec2(label_width, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.set_width(label_width);
            label(ui);
        });
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), size::CONTROL_H),
            Layout::top_down(Align::Max),
            control,
        );
    });
}
