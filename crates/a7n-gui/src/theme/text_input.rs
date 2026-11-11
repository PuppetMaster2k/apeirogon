// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui::{Margin, Response, RichText, TextEdit, Ui, WidgetText};

use super::{Palette, font_medium, font_mono, font_regular};

/// Labelled single-line input. `mono` for addresses, codes, IDs.
pub(crate) fn text_input(
    ui: &mut Ui,
    label: &str,
    value: &mut String,
    hint: &str,
    mono: bool,
) -> Response {
    masked_text_input(ui, label, value, hint, mono, false)
}

pub(crate) fn masked_text_input(
    ui: &mut Ui,
    label: &str,
    value: &mut String,
    hint: &str,
    mono: bool,
    hidden: bool,
) -> Response {
    let p = Palette::of(ui.ctx());
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(RichText::new(label).font(font_medium(13.0)).color(p.text));
        let font = if mono {
            font_mono(13.0)
        } else {
            font_regular(14.0)
        };
        ui.add(
            TextEdit::singleline(value)
                .password(hidden)
                .hint_text(WidgetText::from(RichText::new(hint).color(p.text3)))
                .font(font)
                .margin(Margin::symmetric(10, 7))
                .desired_width(f32::INFINITY),
        )
    })
    .inner
}
