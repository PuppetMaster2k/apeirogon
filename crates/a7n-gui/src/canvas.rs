// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::layout::{LayoutDraft, LayoutDraftError};
use a7n_ipc::{LayoutInfo, LayoutPlacement, Placement, validate};
use eframe::egui;

use crate::theme;

const CANVAS_HEIGHT: f32 = 350.0;
const CANVAS_PADDING: f32 = 20.0;
const MOVE_STEP: i32 = 100;
const FINE_MOVE_STEP: i32 = 10;

pub(super) struct Canvas {
    draft: LayoutDraft,
    selected: usize,
    bounds: [f64; 4],
}

impl TryFrom<LayoutInfo> for Canvas {
    type Error = LayoutDraftError;
    fn try_from(info: LayoutInfo) -> Result<Self, Self::Error> {
        let draft = LayoutDraft::try_from(info)?;
        let bounds = bounds(&draft.placements);
        Ok(Self {
            draft,
            selected: 0,
            bounds,
        })
    }
}

impl Canvas {
    pub(super) fn fit(&mut self) {
        self.bounds = bounds(&self.draft.placements);
    }

    pub(super) fn is_valid(&self) -> bool {
        validate(&self.draft.placements).is_ok()
    }

    pub(super) fn placed(&self) -> Vec<LayoutPlacement> {
        self.draft.placed()
    }

    pub(super) fn show(&mut self, ui: &mut egui::Ui) {
        theme::small(
            ui,
            "Drag machines to place them. Tab selects a machine; arrow keys move it (Shift for fine movement). Edges connect where rectangles touch.",
        );
        if self.draft.lost_edit {
            theme::badge(
                ui,
                theme::BadgeKind::Warning,
                "Another machine replaced your last layout edit.",
            );
        }
        let (area, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), CANVAS_HEIGHT),
            egui::Sense::hover(),
        );
        let transform = Transform::new(self.bounds, area.shrink(CANVAS_PADDING));
        for (index, (placement, name)) in self
            .draft
            .placements
            .iter_mut()
            .zip(&self.draft.names)
            .enumerate()
        {
            let rect = transform.rect(placement);
            let response = ui.interact(rect, ui.id().with(index), egui::Sense::click_and_drag());
            paint_machine(ui, rect, name, placement, self.selected == index);
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Button,
                    ui.is_enabled(),
                    self.selected == index,
                    name,
                )
            });
            if response.clicked() || response.drag_started() {
                response.request_focus();
                self.selected = index;
            }
            if response.has_focus() {
                self.selected = index;
                ui.memory_mut(|memory| {
                    memory.set_focus_lock_filter(
                        response.id,
                        egui::EventFilter {
                            horizontal_arrows: true,
                            vertical_arrows: true,
                            ..Default::default()
                        },
                    );
                });
                ui.input(|input| {
                    let step = if input.modifiers.shift {
                        FINE_MOVE_STEP
                    } else {
                        MOVE_STEP
                    };
                    for (key, dx, dy) in [
                        (egui::Key::ArrowLeft, -step, 0),
                        (egui::Key::ArrowRight, step, 0),
                        (egui::Key::ArrowUp, 0, -step),
                        (egui::Key::ArrowDown, 0, step),
                    ] {
                        if input.key_pressed(key) {
                            placement.x = placement.x.saturating_add(dx);
                            placement.y = placement.y.saturating_add(dy);
                        }
                    }
                });
            }
            if response.dragged() {
                let delta = ui.input(|input| input.pointer.delta());
                move_placement(placement, delta, transform.scale);
            }
        }
        let validation = validate(&self.draft.placements);
        if let Err(error) = &validation {
            theme::error(ui, &error.to_string());
        }
    }
}

fn paint_machine(
    ui: &egui::Ui,
    rect: egui::Rect,
    name: &str,
    placement: &Placement,
    selected: bool,
) {
    let palette = theme::Palette::of(ui.ctx());
    ui.painter().rect(
        rect,
        theme::size::R_CARD,
        if selected {
            palette.accent_subtle
        } else {
            palette.surface
        },
        egui::Stroke::new(
            1.0,
            if selected {
                palette.accent
            } else {
                palette.border
            },
        ),
        egui::StrokeKind::Inside,
    );
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.text(
        rect.center() - egui::vec2(0.0, 10.0),
        egui::Align2::CENTER_CENTER,
        name,
        theme::font_medium(14.0),
        palette.text,
    );
    painter.text(
        rect.center() + egui::vec2(0.0, 10.0),
        egui::Align2::CENTER_CENTER,
        format!("({}, {})", placement.x, placement.y),
        theme::font_mono(12.0),
        palette.text3,
    );
}

struct Transform {
    origin: egui::Pos2,
    minimum: [f64; 2],
    scale: f64,
}

impl Transform {
    fn new(bounds: [f64; 4], area: egui::Rect) -> Self {
        let scale = (f64::from(area.width()) / (bounds[2] - bounds[0]).max(1.0))
            .min(f64::from(area.height()) / (bounds[3] - bounds[1]).max(1.0));
        Self {
            origin: area.min,
            minimum: [bounds[0], bounds[1]],
            scale: scale.max(f64::MIN_POSITIVE),
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "canvas coordinates are bounded screen pixels"
    )]
    fn rect(&self, p: &Placement) -> egui::Rect {
        let x = self.origin.x + ((f64::from(p.x) - self.minimum[0]) * self.scale) as f32;
        let y = self.origin.y + ((f64::from(p.y) - self.minimum[1]) * self.scale) as f32;
        egui::Rect::from_min_size(
            egui::pos2(x, y),
            egui::vec2(
                (f64::from(p.width) * self.scale) as f32,
                (f64::from(p.height) * self.scale) as f32,
            ),
        )
    }
}

fn bounds(placements: &[Placement]) -> [f64; 4] {
    if placements.is_empty() {
        return [0.0, 0.0, 1.0, 1.0];
    }
    placements.iter().fold(
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        |b, p| {
            [
                b[0].min(f64::from(p.x)),
                b[1].min(f64::from(p.y)),
                b[2].max(f64::from(p.x) + f64::from(p.width)),
                b[3].max(f64::from(p.y) + f64::from(p.height)),
            ]
        },
    )
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "float-to-int casts saturate at the layout coordinate limits"
)]
fn move_placement(p: &mut Placement, delta: egui::Vec2, scale: f64) {
    p.x = (f64::from(p.x) + f64::from(delta.x) / scale).round() as i32;
    p.y = (f64::from(p.y) + f64::from(delta.y) / scale).round() as i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_negative_coordinates_fit_and_drag_in_layout_units() {
        // Arrange
        let mut placement = Placement {
            id: "0123456789abcdef0123456789abcdef".parse().unwrap(),
            x: -1920,
            y: -100,
            width: 1920,
            height: 1080,
        };
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 540.0));
        let transform = Transform::new(bounds(&[placement]), area);

        // Act
        let rect = transform.rect(&placement);
        move_placement(&mut placement, egui::vec2(100.0, -50.0), transform.scale);

        // Assert
        assert_eq!(rect, area);
        assert_eq!((placement.x, placement.y), (-1720, -200));
    }
}
