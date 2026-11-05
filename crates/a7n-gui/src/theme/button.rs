// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui;
use eframe::egui::{
    Color32, CornerRadius, CursorIcon, Response, Sense, Stroke, StrokeKind, Ui, WidgetInfo,
    WidgetType, vec2,
};

use super::paint::{fade, focus_ring};
use super::{Palette, font_icon, font_medium, size};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ButtonKind {
    /// One per view: the main action (Save, Show pairing code, Start daemon).
    Primary,
    /// Everything else that is a real action.
    Secondary,
    /// Toolbars, row actions, Cancel.
    Ghost,
    /// Destructive row actions (Unpair) — quiet until hovered.
    DangerQuiet,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ButtonSize {
    Md,
    Sm,
}

struct Paint {
    fill: Color32,
    stroke: Color32,
    text: Color32,
}

fn button_paint(p: &Palette, kind: ButtonKind, hovered: bool, pressed: bool) -> Paint {
    use ButtonKind::{DangerQuiet, Ghost, Primary, Secondary};
    let t = Color32::TRANSPARENT;
    match (kind, pressed, hovered) {
        (Primary, true, _) => Paint {
            fill: p.accent_press,
            stroke: p.accent_press,
            text: p.on_accent,
        },
        (Primary, _, true) => Paint {
            fill: p.accent_hover,
            stroke: p.accent_hover,
            text: p.on_accent,
        },
        (Primary, _, _) => Paint {
            fill: p.accent,
            stroke: p.accent,
            text: p.on_accent,
        },
        (Secondary, true, _) => Paint {
            fill: p.pressed,
            stroke: p.border_strong,
            text: p.text,
        },
        (Secondary, _, true) => Paint {
            fill: p.hover,
            stroke: p.border_strong,
            text: p.text,
        },
        (Secondary, _, _) => Paint {
            fill: p.surface,
            stroke: p.border,
            text: p.text,
        },
        (Ghost, true, _) => Paint {
            fill: p.pressed,
            stroke: t,
            text: p.text,
        },
        (Ghost, _, true) => Paint {
            fill: p.hover,
            stroke: t,
            text: p.text,
        },
        (Ghost, _, _) => Paint {
            fill: t,
            stroke: t,
            text: p.text,
        },
        (DangerQuiet, true, _) => Paint {
            fill: p.danger_subtle,
            stroke: p.danger,
            text: p.danger,
        },
        (DangerQuiet, _, true) => Paint {
            fill: p.danger_subtle,
            stroke: t,
            text: p.danger,
        },
        (DangerQuiet, _, _) => Paint {
            fill: t,
            stroke: t,
            text: p.danger,
        },
    }
}

/// Text button with optional leading Phosphor icon.
///
/// ```ignore
/// if theme::button(ui, ButtonKind::Primary, ButtonSize::Md, Some(ph::FLOPPY_DISK), "Save changes").clicked() { … }
/// ```
pub(crate) fn button(
    ui: &mut Ui,
    kind: ButtonKind,
    sz: ButtonSize,
    icon: Option<&str>,
    label: &str,
) -> Response {
    let p = Palette::of(ui.ctx());
    let (h, font, icon_size, pad_x) = match sz {
        ButtonSize::Md => (size::CONTROL_H, font_medium(14.0), size::ICON, 14.0),
        ButtonSize::Sm => (size::CONTROL_H_SM, font_medium(12.0), 14.0, 10.0),
    };
    let painter = ui.painter().clone();
    let text = painter.layout_no_wrap(label.to_owned(), font, Color32::PLACEHOLDER);
    let icon_g = icon
        .map(|i| painter.layout_no_wrap(i.to_owned(), font_icon(icon_size), Color32::PLACEHOLDER));
    let gap = 8.0;
    let icon_w = icon_g.as_ref().map_or(0.0, |g| g.size().x + gap);
    let pad_left = if icon.is_some() { pad_x - 2.0 } else { pad_x };
    let width = pad_left + icon_w + text.size().x + pad_x;

    let (rect, resp) = ui.allocate_exact_size(vec2(width, h), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label));

    if ui.is_rect_visible(rect) {
        let enabled = ui.is_enabled();
        let pressed = enabled && resp.is_pointer_button_down_on();
        let hovered = enabled && resp.hovered();
        let c = button_paint(p, kind, hovered, pressed);
        painter.rect(
            rect,
            CornerRadius::same(size::R_CONTROL),
            fade(c.fill, enabled),
            Stroke::new(1.0, fade(c.stroke, enabled)),
            StrokeKind::Inside,
        );
        let mut x = rect.left() + pad_left;
        if let Some(g) = icon_g {
            let pos = egui::pos2(x, rect.center().y - g.size().y / 2.0);
            x += g.size().x + gap;
            painter.galley(pos, g, fade(c.text, enabled));
        }
        let pos = egui::pos2(x, rect.center().y - text.size().y / 2.0);
        painter.galley(pos, text, fade(c.text, enabled));
        if resp.has_focus() {
            focus_ring(ui, rect, size::R_CONTROL);
        }
    }
    if ui.is_enabled() {
        resp.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        resp
    }
}
