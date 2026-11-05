// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui;
use eframe::egui::{Response, Sense, Stroke, Ui, WidgetInfo, WidgetType, vec2};

use super::{Palette, font_medium, size};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BadgeKind {
    Online,
    Offline,
    Accent,
    Warning,
    Danger,
}

/// Status pill. Always carries text (never color alone).
pub(crate) fn badge(ui: &mut Ui, kind: BadgeKind, text: &str) -> Response {
    let p = Palette::of(ui.ctx());
    let (bg, fg, dot) = match kind {
        BadgeKind::Online => (p.success_subtle, p.success, true),
        BadgeKind::Offline => (p.hover, p.text2, true),
        BadgeKind::Accent => (p.accent_subtle, p.accent_text, false),
        BadgeKind::Warning => (p.warning_subtle, p.warning, true),
        BadgeKind::Danger => (p.danger_subtle, p.danger, true),
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font_medium(12.0), fg);
    let dot_width = if dot { 12.0 } else { 0.0 };
    let (rect, response) = ui.allocate_exact_size(
        vec2(galley.size().x + 16.0 + dot_width, 22.0),
        Sense::hover(),
    );
    ui.painter().rect_filled(rect, size::R_PILL, bg);
    if dot {
        let center = egui::pos2(rect.left() + 10.0, rect.center().y);
        if kind == BadgeKind::Offline {
            ui.painter()
                .circle_stroke(center, 2.5, Stroke::new(1.5, p.text3));
        } else {
            ui.painter().circle_filled(center, 3.0, fg);
        }
    }
    ui.painter().galley(
        egui::pos2(
            rect.left() + 8.0 + dot_width,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        fg,
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, ui.is_enabled(), text));
    response
}
