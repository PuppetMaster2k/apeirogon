// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui;
use eframe::egui::{
    Align2, Color32, CornerRadius, CursorIcon, Response, Sense, Ui, WidgetInfo, WidgetType, vec2,
};

use super::paint::focus_ring;
use super::{Palette, font_icon, font_medium, font_regular, size};

/// Left-hand navigation entry ("tab"). Full width of the sidebar, 36 px tall.
pub(crate) fn nav_item(ui: &mut Ui, icon: &str, label: &str, selected: bool) -> Response {
    let p = Palette::of(ui.ctx());
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), size::NAV_ITEM_H), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, selected, label));
    if ui.is_rect_visible(rect) {
        let hovered = resp.hovered();
        let (fill, fg) = if selected {
            (p.accent_subtle, p.accent_text)
        } else if hovered {
            (p.hover, p.text)
        } else {
            (Color32::TRANSPARENT, p.text2)
        };
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(size::R_CARD), fill);
        let y = rect.center().y;
        painter.text(
            egui::pos2(rect.left() + 10.0, y),
            Align2::LEFT_CENTER,
            icon,
            font_icon(size::ICON),
            fg,
        );
        let font = if selected {
            font_medium(14.0)
        } else {
            font_regular(14.0)
        };
        let text = egui::WidgetText::from(label).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            (rect.width() - 48.0).max(0.0),
            font,
        );
        painter.galley(
            egui::pos2(rect.left() + 38.0, y - text.size().y / 2.0),
            text,
            fg,
        );
        if resp.has_focus() {
            focus_ring(ui, rect, size::R_CARD);
        }
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}
