// SPDX-License-Identifier: GPL-3.0-or-later

use std::borrow::Cow;

use a7n_platform::feedback::Color;
use eframe::egui;
use rust_i18n::t;

use crate::{App, theme};

impl App {
    pub(super) fn feedback_color(&mut self, ui: &mut egui::Ui) {
        let current = self.value("feedback.color", "accent");
        let (selected, _, name) = Color::choices()
            .find(|(_, key, _)| *key == current)
            .unwrap_or((Color::Accent, "accent", "Accent"));
        theme::setting_row(ui, &t!("Colour"), "", Some("feedback.color"), |ui| {
            egui::ComboBox::from_id_salt("feedback.color")
                .selected_text(color_name(selected, name))
                .show_ui(ui, |ui| {
                    for (color, key, name) in Color::choices() {
                        ui.horizontal(|ui| {
                            swatch(ui, color);
                            if ui
                                .selectable_label(key == current, color_name(color, name))
                                .clicked()
                            {
                                self.edit("feedback.color", key, "accent");
                            }
                        });
                    }
                });
            swatch(ui, selected);
        });
    }
}

fn color_name(color: Color, name: &'static str) -> Cow<'static, str> {
    match color {
        Color::Accent => t!("Accent"),
        Color::Green => t!("Green"),
        Color::Orange => t!("Orange"),
        Color::Pink => t!("Pink"),
        _ => Cow::Borrowed(name),
    }
}

fn swatch(ui: &mut egui::Ui, color: Color) {
    const WIDTH: f32 = 40.0;
    const HEIGHT: f32 = 14.0;
    const SEGMENTS: u8 = 32;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(WIDTH, HEIGHT), egui::Sense::hover());
    for segment in 0..SEGMENTS {
        let t = f32::from(segment) / f32::from(SEGMENTS - 1);
        let [r, g, b] = color.sample(t, ui.visuals().dark_mode);
        let left = rect.left() + f32::from(segment) * WIDTH / f32::from(SEGMENTS);
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(left, rect.top()),
                egui::pos2(left + WIDTH / f32::from(SEGMENTS), rect.bottom()),
            ),
            0.0,
            egui::Color32::from_rgb(r, g, b),
        );
    }
}
