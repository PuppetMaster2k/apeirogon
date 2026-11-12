// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui::{self, Align2, Frame, Margin, Sense, Stroke, StrokeKind, Ui};

use super::{Palette, font_icon, font_medium, font_regular, icon, size};

const HEADER_HEIGHT: f32 = 52.0;
const BODY_INDENT: i8 = 40;

pub(crate) fn accordion_group<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = Palette::of(ui.ctx());
    Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(size::R_CARD)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui)
        })
        .inner
}

#[allow(
    clippy::too_many_arguments,
    reason = "matches the documented accordion component API"
)]
pub(crate) fn accordion<R>(
    ui: &mut Ui,
    id: impl egui::AsIdSalt,
    title: &str,
    summary: Option<&str>,
    default_open: bool,
    disabled: bool,
    add: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    ui.add_enabled_ui(!disabled, |ui| {
        let id = ui.make_persistent_id(id);
        let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            id,
            default_open,
        );
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), HEADER_HEIGHT),
            Sense::click(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::CollapsingHeader, ui.is_enabled(), title)
        });
        if response.clicked() {
            state.toggle(ui);
        }
        let p = Palette::of(ui.ctx());
        let color = |color| super::paint::fade(color, ui.is_enabled());
        if response.hovered() {
            ui.painter().rect_filled(rect, 0, color(p.hover));
        }
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                0,
                Stroke::new(2.0, color(p.accent)),
                StrokeKind::Inside,
            );
        }
        ui.painter().text(
            rect.left_center() + egui::vec2(20.0, 0.0),
            Align2::CENTER_CENTER,
            if state.is_open() {
                icon::CARET_DOWN
            } else {
                icon::CARET_RIGHT
            },
            font_icon(16.0),
            color(p.text2),
        );
        ui.painter().text(
            rect.left_center() + egui::vec2(f32::from(BODY_INDENT), 0.0),
            Align2::LEFT_CENTER,
            title,
            font_medium(14.0),
            color(p.text),
        );
        if let Some(summary) = summary {
            let available = (rect.width()
                - 60.0
                - ui.fonts_mut(|fonts| {
                    fonts
                        .layout_no_wrap(title.to_owned(), font_medium(14.0), p.text)
                        .size()
                        .x
                }))
            .max(0.0);
            let mut job = egui::text::LayoutJob::simple_singleline(
                summary.to_owned(),
                font_regular(12.0),
                color(p.text3),
            );
            job.wrap.max_width = available;
            job.wrap.max_rows = 1;
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            ui.painter().galley(
                rect.right_center() - egui::vec2(20.0 + galley.size().x, galley.size().y / 2.0),
                galley,
                color(p.text3),
            );
        }
        let result = state
            .show_body_unindented(ui, |ui| {
                Frame::new()
                    .inner_margin(Margin {
                        left: BODY_INDENT,
                        right: 20,
                        top: 0,
                        bottom: 20,
                    })
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = size::ROW_GAP;
                        add(ui)
                    })
                    .inner
            })
            .map(|response| response.inner);
        ui.separator();
        result
    })
    .inner
}
