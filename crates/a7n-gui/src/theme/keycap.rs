// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui::{
    Align, CornerRadius, Frame, Layout, Margin, Response, RichText, Stroke, Ui, vec2,
};

use super::{Palette, font_mono, size};

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
        .stroke(Stroke::new(1.0, p.border_strong))
        .corner_radius(CornerRadius::same(size::R_KEYCAP))
        .inner_margin(Margin::symmetric(7, 2))
        .show(ui, |ui| {
            ui.label(
                RichText::new(key_name(key))
                    .font(font_mono(12.0))
                    .color(p.text),
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
                .layout_no_wrap(key_name(key), font_mono(12.0), Palette::of(ui.ctx()).text)
                .size()
                .x
                + 18.0
        })
        .sum();
    ui.allocate_ui_with_layout(
        vec2(width.min(ui.available_width()), size::CONTROL_H),
        Layout::left_to_right(Align::Center).with_main_wrap(true),
        |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for key in keys {
                keycap(ui, key);
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_name_config_tokens_use_consistent_human_labels() {
        // Arrange
        let meta = match std::env::consts::OS {
            "windows" => "Win",
            "macos" => "Cmd",
            _ => "Super",
        };
        let cases = [
            ("right_alt", "R Alt".to_owned()),
            ("scroll_lock", "Scroll Lock".to_owned()),
            ("home", "Home".to_owned()),
            ("ctrl", "Ctrl".to_owned()),
            ("l", "L".to_owned()),
            ("left_ctrl", "L Ctrl".to_owned()),
            ("page_up", "Page Up".to_owned()),
            ("f12", "F12".to_owned()),
            ("meta", meta.to_owned()),
            ("left_meta", format!("L {meta}")),
            ("right_meta", format!("R {meta}")),
            ("super", meta.to_owned()),
            ("win", meta.to_owned()),
        ];

        for (key, expected) in cases {
            // Act
            let actual = key_name(key);

            // Assert
            assert_eq!(actual, expected, "{key}");
        }
    }
}
