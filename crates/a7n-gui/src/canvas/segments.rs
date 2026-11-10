// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::{Edge, Placement, third_at};
use eframe::egui;

use crate::theme;

const EDGE_HIT_PX: f32 = 8.0;
const SEGMENT_WIDTH_PX: f32 = 4.0;
const EDGES: [Edge; 4] = [Edge::Left, Edge::Top, Edge::Right, Edge::Bottom];

fn hit_test(rect: egui::Rect, pointer: egui::Pos2) -> Option<(Edge, u8)> {
    if !rect.contains(pointer) || !rect.is_positive() {
        return None;
    }
    let distances = [
        pointer.x - rect.left(),
        pointer.y - rect.top(),
        rect.right() - pointer.x,
        rect.bottom() - pointer.y,
    ];
    let (index, distance) = distances
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.total_cmp(b.1))?;
    if *distance > EDGE_HIT_PX {
        return None;
    }
    let edge = EDGES[index];
    let along = if matches!(edge, Edge::Left | Edge::Right) {
        (pointer.y - rect.top()) / rect.height()
    } else {
        (pointer.x - rect.left()) / rect.width()
    };
    Some((edge, third_at(along)))
}

pub(super) fn interact(
    ui: &egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
    placement: &mut Placement,
) -> Option<(Edge, u8)> {
    if response.clicked()
        && let Some((edge, third)) = response
            .interact_pointer_pos()
            .and_then(|p| hit_test(rect, p))
    {
        placement.toggle_blocked(edge, third);
    }
    if !response.hovered() || response.dragged() {
        return None;
    }
    let segment = ui
        .input(|input| input.pointer.hover_pos())
        .and_then(|p| hit_test(rect, p))?;
    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    Some(segment)
}

pub(super) fn paint(
    ui: &egui::Ui,
    rect: egui::Rect,
    placement: &Placement,
    hovered: Option<(Edge, u8)>,
) {
    let palette = theme::Palette::of(ui.ctx());
    for edge in EDGES {
        for third in 0..3 {
            let focused = hovered == Some((edge, third));
            let blocked = placement.is_blocked(edge, third);
            if !blocked && !focused {
                continue;
            }
            let (start, end) = match edge {
                Edge::Left => (rect.left_top(), rect.left_bottom()),
                Edge::Top => (rect.left_top(), rect.right_top()),
                Edge::Right => (rect.right_top(), rect.right_bottom()),
                Edge::Bottom => (rect.left_bottom(), rect.right_bottom()),
                Edge::Unspecified => continue,
            };
            let points =
                [third, third + 1].map(|part| start + (end - start) * (f32::from(part) / 3.0));
            if focused {
                ui.painter().line_segment(
                    points,
                    egui::Stroke::new(SEGMENT_WIDTH_PX * 2.0, palette.accent),
                );
            }
            if blocked {
                ui.painter()
                    .line_segment(points, egui::Stroke::new(SEGMENT_WIDTH_PX, palette.danger));
            }
        }
    }
}
