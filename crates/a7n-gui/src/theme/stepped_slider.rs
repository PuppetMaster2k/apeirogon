// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui::{self, Align2, Response, Stroke, Ui};

use super::{Palette, font_mono, font_regular, slider::track};

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "indices are bounded by the supplied UI options"
)]
pub(crate) fn stepped_slider(
    ui: &mut Ui,
    label: &str,
    value: &mut u32,
    steps: &[(u32, &str)],
) -> Response {
    assert!(
        steps.len() >= 2,
        "a stepped slider needs at least two options"
    );
    let mut index = steps
        .iter()
        .position(|(option, _)| option == value)
        .unwrap_or(0) as f64;
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.label(label);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(value.to_string()).font(font_mono(13.0)));
            });
        });
        ui.spacing_mut().slider_width = ui.available_width();
        let response = track(
            ui,
            label,
            &mut index,
            0.0..=(steps.len() - 1) as f64,
            1.0,
            |index| steps[index as usize].0.to_string(),
        );
        if response.changed() {
            *value = steps[index as usize].0;
        }
        let p = Palette::of(ui.ctx());
        let rect = response.rect.shrink2(egui::vec2(8.0, 0.0));
        for (index, (_, label)) in steps.iter().enumerate() {
            let x = egui::lerp(rect.x_range(), index as f32 / (steps.len() - 1) as f32);
            let top = egui::pos2(x, rect.bottom());
            ui.painter().line_segment(
                [top, top + egui::vec2(0.0, 4.0)],
                Stroke::new(1.0, p.border_strong),
            );
            ui.painter().text(
                top + egui::vec2(0.0, 8.0),
                Align2::CENTER_TOP,
                label,
                font_regular(12.0),
                p.text3,
            );
        }
        ui.add_space(24.0);
        response
    })
    .inner
}
