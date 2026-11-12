// SPDX-License-Identifier: GPL-3.0-or-later

use std::ops::RangeInclusive;

use eframe::egui::{self, Align2, EventFilter, Key, Response, Sense, Stroke, Ui};

use super::{Palette, font_mono, paint, size};

const THUMB_RADIUS: f32 = 8.0;
const TRACK_WIDTH: f32 = 4.0;
const DEFAULT_STEP: f64 = 0.01;
const VALUE_WIDTH: f32 = 64.0;

pub(crate) fn slider(
    ui: &mut Ui,
    label: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
    step: Option<f64>,
    format: impl Fn(f64) -> String,
) -> Response {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.spacing_mut().slider_width =
            (ui.available_width() - VALUE_WIDTH - ui.spacing().item_spacing.x)
                .max(THUMB_RADIUS * 2.0 + 1.0);
        let response = track(
            ui,
            label,
            value,
            range,
            step.unwrap_or(DEFAULT_STEP),
            &format,
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(format(*value)).font(font_mono(13.0)));
        });
        response
    })
    .inner
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "slider geometry uses display precision"
)]
pub(super) fn track(
    ui: &mut Ui,
    label: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
    step: f64,
    format: impl Fn(f64) -> String,
) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(
        egui::vec2(ui.spacing().slider_width, size::CONTROL_H),
        Sense::click_and_drag(),
    );
    let start = *range.start();
    let end = *range.end();
    let old = *value;
    let rail = rect.shrink2(egui::vec2(THUMB_RADIUS, 0.0));
    if let Some(pointer) = response.interact_pointer_pos() {
        response.request_focus();
        *value = start
            + f64::from(((pointer.x - rail.left()) / rail.width()).clamp(0.0, 1.0)) * (end - start);
        *value = (start + ((*value - start) / step).round() * step).clamp(start, end);
    }
    if response.has_focus() && ui.is_enabled() {
        ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                response.id,
                EventFilter {
                    horizontal_arrows: true,
                    ..EventFilter::default()
                },
            );
        });
        ui.input_mut(|input| {
            let increment = step * if input.modifiers.shift { 10.0 } else { 1.0 };
            if input.consume_key(egui::Modifiers::NONE, Key::Home) {
                *value = start;
            }
            if input.consume_key(egui::Modifiers::NONE, Key::End) {
                *value = end;
            }
            for (key, sign) in [(Key::ArrowLeft, -1.0), (Key::ArrowRight, 1.0)] {
                let modifiers = input.modifiers;
                if input.consume_key(modifiers, key) {
                    *value = (*value + sign * increment).clamp(start, end);
                }
            }
        });
    }
    if value.to_bits() != old.to_bits() {
        response.mark_changed();
    }
    response.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), *value, label));
    draw(
        ui,
        &response,
        rail,
        ((*value - start) / (end - start)) as f32,
        &format(*value),
    );
    response
}

fn draw(ui: &Ui, response: &Response, rail: egui::Rect, fraction: f32, value: &str) {
    let p = Palette::of(ui.ctx());
    let painter = ui.painter();
    let thumb = egui::pos2(egui::lerp(rail.x_range(), fraction), rail.center().y);
    let color = |color| paint::fade(color, ui.is_enabled());
    painter.line_segment(
        [rail.left_center(), rail.right_center()],
        Stroke::new(TRACK_WIDTH, color(p.border_strong)),
    );
    painter.line_segment(
        [rail.left_center(), thumb],
        Stroke::new(TRACK_WIDTH, color(p.accent)),
    );
    if response.hovered() {
        painter.circle_filled(thumb, THUMB_RADIUS + 6.0, color(p.focus));
    }
    if response.has_focus() {
        painter.circle_stroke(thumb, THUMB_RADIUS + 2.0, Stroke::new(3.0, color(p.focus)));
    }
    painter.circle_filled(thumb, THUMB_RADIUS, color(super::LIGHT.surface));
    painter.circle_stroke(
        thumb,
        THUMB_RADIUS,
        if response.dragged() {
            Stroke::new(2.0, color(p.accent))
        } else {
            Stroke::new(1.0, color(p.border_strong))
        },
    );
    if response.dragged() {
        let bubble =
            egui::Rect::from_center_size(thumb - egui::vec2(0.0, 28.0), egui::vec2(72.0, 24.0));
        painter.rect_filled(bubble, size::R_CONTROL, p.surface);
        painter.text(
            bubble.center(),
            Align2::CENTER_CENTER,
            value,
            font_mono(12.0),
            p.text,
        );
    }
}
