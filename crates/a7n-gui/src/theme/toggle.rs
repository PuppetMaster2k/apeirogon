// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui;
use eframe::egui::{Color32, CornerRadius, Response, Sense, Ui, WidgetInfo, WidgetType, vec2};

use super::paint::{fade, focus_ring};
use super::{Palette, size};

/// iOS-style switch for booleans (`allow_drive`, start with OS …).
pub(crate) fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let p = Palette::of(ui.ctx());
    // Allocate the full control height so rows align, draw the 36×20 track centered.
    let (rect, mut resp) = ui.allocate_exact_size(vec2(36.0, size::CONTROL_H), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), *on, ""));
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(resp.id, *on);
        let track = egui::Rect::from_center_size(rect.center(), vec2(36.0, 20.0));
        let fill = if *on {
            if resp.hovered() {
                p.accent_hover
            } else {
                p.accent
            }
        } else if resp.hovered() {
            p.text3
        } else {
            p.border_strong
        };
        let painter = ui.painter();
        painter.rect_filled(track, CornerRadius::same(10), fade(fill, ui.is_enabled()));
        let x = egui::lerp((track.left() + 10.0)..=(track.right() - 10.0), t);
        painter.circle_filled(egui::pos2(x, track.center().y), 8.0, Color32::WHITE);
        if resp.has_focus() {
            focus_ring(ui, track, 10);
        }
    }
    resp
}
