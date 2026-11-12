// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui::{self, Align2, Frame, Margin, Sense, Stroke, StrokeKind, Ui};

use super::{Palette, font_icon, font_medium, font_regular, icon, size};

const HEADER_HEIGHT: f32 = 52.0;
const BODY_INDENT: i8 = 40;

#[derive(Clone)]
struct HeaderPaint {
    response: egui::Response,
    background: egui::layers::ShapeIdx,
    enabled: bool,
}

impl HeaderPaint {
    fn paint(&self, ui: &Ui, rect: egui::Rect, radius: egui::CornerRadius) {
        let p = Palette::of(ui.ctx());
        let fill = if self.response.hovered() {
            super::paint::fade(p.hover, self.enabled)
        } else {
            egui::Color32::TRANSPARENT
        };
        let stroke = if self.response.has_focus() {
            Stroke::new(2.0, super::paint::fade(p.accent, self.enabled))
        } else {
            Stroke::NONE
        };
        ui.painter().set(
            self.background,
            egui::epaint::RectShape::new(rect, radius, fill, stroke, StrokeKind::Inside),
        );
    }
}

fn headers_id() -> egui::Id {
    egui::Id::new("accordion_header_paints")
}

pub(crate) fn accordion_group<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = Palette::of(ui.ctx());
    ui.data_mut(|data| data.insert_temp(headers_id(), Vec::<HeaderPaint>::new()));
    // Defer header backgrounds until the card has its final bounds.
    let response = Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(size::R_CARD)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui)
        });
    let headers = ui
        .data_mut(|data| data.remove_temp::<Vec<HeaderPaint>>(headers_id()))
        .unwrap_or_default();
    let card = response.response.rect.shrink(1.0);
    for header in headers {
        let rect = egui::Rect::from_min_max(
            egui::pos2(card.left(), header.response.rect.top()),
            egui::pos2(card.right(), header.response.rect.bottom()),
        );
        let top = if rect.top() <= card.top() {
            size::R_CARD
        } else {
            0
        };
        let bottom = if rect.bottom() >= card.bottom() {
            size::R_CARD
        } else {
            0
        };
        let radius = egui::CornerRadius {
            nw: top,
            ne: top,
            sw: bottom,
            se: bottom,
        };
        header.paint(ui, rect, radius);
    }
    response.inner
}

fn header_background(ui: &Ui, response: &egui::Response) {
    let header = HeaderPaint {
        response: response.clone(),
        background: ui.painter().add(egui::Shape::Noop),
        enabled: ui.is_enabled(),
    };
    debug_assert!(
        ui.data(|data| data.get_temp::<Vec<HeaderPaint>>(headers_id()).is_some()),
        "accordions must be direct children of an accordion_group"
    );
    ui.data_mut(|data| {
        data.get_temp_mut_or_default::<Vec<HeaderPaint>>(headers_id())
            .push(header);
    });
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
        if ui.data(|data| {
            data.get_temp::<Vec<HeaderPaint>>(headers_id())
                .is_some_and(|headers| !headers.is_empty())
        }) {
            ui.separator();
        }
        let response = ui.allocate_response(
            egui::vec2(ui.available_width(), HEADER_HEIGHT),
            Sense::click(),
        );
        let rect = response.rect;
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::CollapsingHeader, ui.is_enabled(), title)
        });
        if response.clicked() {
            state.toggle(ui);
        }
        let p = Palette::of(ui.ctx());
        let color = |color| super::paint::fade(color, ui.is_enabled());
        header_background(ui, &response);
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
        state
            .show_body_unindented(ui, |ui| {
                Frame::new()
                    .inner_margin(Margin {
                        left: BODY_INDENT,
                        right: 20,
                        top: 8,
                        bottom: 20,
                    })
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = size::ROW_GAP;
                        add(ui)
                    })
                    .inner
            })
            .map(|response| response.inner)
    })
    .inner
}
