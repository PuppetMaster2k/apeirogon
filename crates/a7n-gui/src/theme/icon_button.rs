// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui::{
    Align2, Color32, CornerRadius, Response, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo,
    WidgetType,
};

use super::paint::{fade, focus_ring};
use super::{Palette, font_icon, size};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum IconButtonKind {
    Ghost,
    Bordered,
    Danger,
}

/// Square icon-only button. Always pass a tooltip: it is also the accessible label.
pub(crate) fn icon_button(
    ui: &mut Ui,
    kind: IconButtonKind,
    icon: &str,
    tooltip: &str,
    selected: bool,
) -> Response {
    let p = Palette::of(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size::CONTROL_H), Sense::click());
    resp.widget_info(|| {
        WidgetInfo::selected(WidgetType::Button, ui.is_enabled(), selected, tooltip)
    });
    if ui.is_rect_visible(rect) {
        let enabled = ui.is_enabled();
        let hovered = enabled && resp.hovered();
        let pressed = enabled && resp.is_pointer_button_down_on();
        let (mut fill, mut stroke, mut fg) = match kind {
            IconButtonKind::Ghost => (Color32::TRANSPARENT, Color32::TRANSPARENT, p.text2),
            IconButtonKind::Bordered => (p.surface, p.border, p.text),
            IconButtonKind::Danger => (Color32::TRANSPARENT, Color32::TRANSPARENT, p.danger),
        };
        if selected {
            fill = p.accent_subtle;
            stroke = Color32::TRANSPARENT;
            fg = p.accent_text;
        } else if pressed {
            fill = if kind == IconButtonKind::Danger {
                p.danger_subtle
            } else {
                p.pressed
            };
        } else if hovered {
            fill = if kind == IconButtonKind::Danger {
                p.danger_subtle
            } else {
                p.hover
            };
            if kind == IconButtonKind::Ghost {
                fg = p.text;
            }
            if kind == IconButtonKind::Bordered {
                stroke = p.border_strong;
            }
        }
        let painter = ui.painter();
        painter.rect(
            rect,
            CornerRadius::same(size::R_CONTROL),
            fade(fill, enabled),
            Stroke::new(1.0, fade(stroke, enabled)),
            StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            icon,
            font_icon(size::ICON),
            fade(fg, enabled),
        );
        if resp.has_focus() {
            focus_ring(ui, rect, size::R_CONTROL);
        }
    }
    resp.on_hover_text(tooltip)
}
