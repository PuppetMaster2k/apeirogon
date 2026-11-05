// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui::{
    Align, Color32, CornerRadius, Frame, Layout, Margin, Response, Sense, Stroke, StrokeKind, Ui,
    WidgetInfo, WidgetType, vec2,
};

use super::paint::focus_ring;
use super::{Palette, font_medium, size};

/// Segmented control for small enums, e.g. clipboard both/send/receive/ask/off.
pub(crate) fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    value: &mut T,
    options: &[(T, &str)],
) -> Response {
    let p = Palette::of(ui.ctx());
    let width: f32 = options
        .iter()
        .map(|(_, label)| {
            ui.painter()
                .layout_no_wrap((*label).to_owned(), font_medium(13.0), p.text)
                .size()
                .x
                + 26.0
        })
        .sum();
    let inner = Frame::new()
        .fill(p.hover)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(size::R_CARD))
        .inner_margin(Margin::same(2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.allocate_ui_with_layout(
                vec2(width, size::CONTROL_H_SM),
                Layout::left_to_right(Align::Center),
                |ui| {
                    let mut changed = false;
                    for (opt, label) in options {
                        let selected = *value == *opt;
                        let font = font_medium(13.0);
                        let g = ui.painter().layout_no_wrap(
                            label.to_string(),
                            font,
                            Color32::PLACEHOLDER,
                        );
                        let (rect, resp) = ui.allocate_exact_size(
                            vec2(g.size().x + 24.0, size::CONTROL_H_SM),
                            Sense::click(),
                        );
                        resp.widget_info(|| {
                            WidgetInfo::selected(WidgetType::RadioButton, true, selected, *label)
                        });
                        if resp.clicked() && !selected {
                            *value = *opt;
                            changed = true;
                        }
                        let painter = ui.painter();
                        if selected {
                            painter.rect(
                                rect,
                                CornerRadius::same(size::R_CONTROL),
                                p.surface,
                                Stroke::new(1.0, p.border),
                                StrokeKind::Inside,
                            );
                        } else if resp.hovered() {
                            painter.rect_filled(
                                rect,
                                CornerRadius::same(size::R_CONTROL),
                                p.pressed,
                            );
                        }
                        let fg = if selected || resp.hovered() {
                            p.text
                        } else {
                            p.text2
                        };
                        painter.galley(rect.center() - g.size() / 2.0, g, fg);
                        if resp.has_focus() {
                            focus_ring(ui, rect, size::R_CONTROL);
                        }
                    }
                    changed
                },
            )
            .inner
        });
    let mut resp = inner.response;
    if inner.inner {
        resp.mark_changed();
    }
    resp
}
