// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::Monitor;
use eframe::egui;

use crate::theme;

const MONITOR_PADDING: f32 = 8.0;

#[allow(
    clippy::cast_precision_loss,
    reason = "monitor geometry needs only display precision"
)]
pub(super) fn paint(ui: &egui::Ui, container: egui::Rect, monitors: &[Monitor]) {
    if monitors.is_empty() {
        return;
    }
    let (width, height) = monitors
        .iter()
        .fold((0_i64, 0_i64), |(width, height), monitor| {
            (
                width.max(i64::from(monitor.x) + i64::from(monitor.width)),
                height.max(i64::from(monitor.y) + i64::from(monitor.height)),
            )
        });
    if width == 0 || height == 0 {
        return;
    }
    let area =
        container.shrink(MONITOR_PADDING.min(container.width().min(container.height()) / 4.0));
    let scale = (area.width() / width as f32).min(area.height() / height as f32);
    let origin = area.center() - egui::vec2(width as f32, height as f32) * scale / 2.0;
    let p = theme::Palette::of(ui.ctx());
    let painter = ui
        .painter()
        .with_clip_rect(container.intersect(ui.clip_rect()));
    for monitor in monitors {
        let rect = egui::Rect::from_min_size(
            origin + egui::vec2(monitor.x as f32, monitor.y as f32) * scale,
            egui::vec2(monitor.width as f32, monitor.height as f32) * scale,
        );
        painter.rect(
            rect,
            theme::size::R_CONTROL,
            p.hover,
            egui::Stroke::new(1.0, p.border),
            egui::StrokeKind::Inside,
        );
    }
}
