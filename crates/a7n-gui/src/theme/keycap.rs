// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui::{
    Align, CornerRadius, Frame, Label, Layout, Margin, Response, RichText, Stroke, Ui, vec2,
};

use super::{Palette, font_mono, size};

const KEYCAP_MARGIN_X: i8 = 7;
const KEYCAP_MARGIN_Y: i8 = 2;
const KEYCAP_SPACING: f32 = 4.0;
const KEYCAP_FONT_SIZE: f32 = 12.0;
const KEYCAP_STROKE: f32 = 1.0;

/// Shared display names for config keys used in chords and modifier remaps.
pub(crate) fn key_name(key: &str) -> String {
    key.trim()
        .to_ascii_lowercase()
        .split('_')
        .map(|part| match part {
            "left" => "L".to_owned(),
            "right" => "R".to_owned(),
            "meta" | "super" | "win" => match std::env::consts::OS {
                "windows" => "Win",
                "macos" => "Cmd",
                _ => "Super",
            }
            .to_owned(),
            other => {
                let mut word = other.to_owned();
                if let Some(first) = word.get_mut(..1) {
                    first.make_ascii_uppercase();
                }
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A key cap ("Ctrl", "Alt", "L") for hotkeys and modifier remaps.
pub(crate) fn keycap(ui: &mut Ui, key: &str) -> Response {
    let p = Palette::of(ui.ctx());
    Frame::new()
        .fill(p.hover)
        .stroke(Stroke::new(KEYCAP_STROKE, p.border_strong))
        .corner_radius(CornerRadius::same(size::R_KEYCAP))
        .inner_margin(Margin::symmetric(KEYCAP_MARGIN_X, KEYCAP_MARGIN_Y))
        .show(ui, |ui| {
            ui.add(
                Label::new(
                    RichText::new(key_name(key))
                        .font(font_mono(KEYCAP_FONT_SIZE))
                        .color(p.text),
                )
                .extend(),
            );
        })
        .response
}

/// A hotkey chord shown as keycaps: "ctrl+alt+shift+l" → [Ctrl] [Alt] [Shift] [L].
pub(crate) fn chord(ui: &mut Ui, chord: &str) {
    let keys: Vec<_> = chord.split('+').collect();
    let width: f32 = keys
        .iter()
        .map(|key| {
            ui.painter()
                .layout_no_wrap(
                    key_name(key),
                    font_mono(KEYCAP_FONT_SIZE),
                    Palette::of(ui.ctx()).text,
                )
                .size()
                .x
                + 2.0 * (f32::from(KEYCAP_MARGIN_X) + KEYCAP_STROKE)
        })
        .sum::<f32>()
        + keys.iter().skip(1).map(|_| KEYCAP_SPACING).sum::<f32>();
    ui.allocate_ui_with_layout(
        vec2(width.min(ui.available_width()), size::CONTROL_H),
        Layout::left_to_right(Align::Center).with_main_wrap(true),
        |ui| {
            ui.spacing_mut().item_spacing.x = KEYCAP_SPACING;
            for key in keys {
                keycap(ui, key);
            }
        },
    );
}
