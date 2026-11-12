// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui::{Align, Label, Layout, Response, RichText, Ui, vec2};

use super::{Palette, overline_style, size, title_style};

pub(crate) const STACK_WIDTH: f32 = 800.0;

/// Uppercase mono section label ("PAIRED MACHINES").
pub(crate) fn overline(ui: &mut Ui, text: &str) -> Response {
    let p = Palette::of(ui.ctx());
    ui.add(
        Label::new(
            RichText::new(text.to_uppercase())
                .text_style(overline_style())
                .color(p.text3)
                .extra_letter_spacing(0.9),
        )
        .truncate(),
    )
}

/// Page title + one-line description on the left, actions on the right.
pub(crate) fn page_header(ui: &mut Ui, title: &str, subtitle: &str, actions: impl FnOnce(&mut Ui)) {
    let p = Palette::of(ui.ctx());
    let heading = |ui: &mut Ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(RichText::new(title).text_style(title_style()).color(p.text));
        ui.label(RichText::new(subtitle).color(p.text2));
    };
    if ui.available_width() < STACK_WIDTH {
        ui.vertical(heading);
        ui.horizontal(|ui| {
            ui.with_layout(
                Layout::right_to_left(Align::Center).with_main_wrap(true),
                actions,
            );
        });
    } else {
        let heading_width = ui.available_width() * 0.48;
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                vec2(heading_width, 0.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(heading_width);
                    heading(ui);
                },
            );
            ui.with_layout(
                Layout::right_to_left(Align::Center).with_main_wrap(true),
                actions,
            );
        });
    }
    ui.add_space(size::SECTION_GAP - 8.0);
}
