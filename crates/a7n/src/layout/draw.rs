// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::{Edge, Placement, shared_edges, validate};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine, Rectangle};
use ratatui::widgets::{Block, Paragraph};

use super::editor::Editor;

pub(super) fn draw(frame: &mut Frame, editor: &Editor) {
    let [canvas_area, status, help] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(2),
        Constraint::Length(2),
    ])
    .areas(frame.area());
    let (x_bounds, y_bounds) = bounds(&editor.placements);
    let edges = shared_edges(&editor.placements);
    let selected = editor.placements.get(editor.selected);
    let canvas = Canvas::default()
        .block(Block::bordered().title(" Desk layout "))
        .x_bounds(x_bounds)
        .y_bounds(y_bounds)
        .paint(|context| {
            for (index, placement) in editor.placements.iter().enumerate() {
                let color = if index == editor.selected {
                    Color::Cyan
                } else {
                    Color::White
                };
                context.draw(&Rectangle {
                    x: f64::from(placement.x),
                    y: -f64::from(placement.y) - f64::from(placement.height),
                    width: f64::from(placement.width),
                    height: f64::from(placement.height),
                    color,
                });
                context.print(
                    f64::from(placement.x),
                    -f64::from(placement.y) - f64::from(placement.height) / 2.0,
                    Span::styled(editor.names[index].clone(), Style::default().fg(color)),
                );
            }
            if let Some(selected) = selected {
                for edge in edges.iter().filter(|edge| edge.from == selected.id) {
                    if let Some(target) = editor.placements.iter().find(|p| p.id == edge.to) {
                        context.draw(&shared_segment(selected, target, edge.edge));
                    }
                }
            }
        });
    frame.render_widget(canvas, canvas_area);
    let mut notices = Vec::new();
    if let Err(error) = validate(&editor.placements) {
        notices.push(Span::styled(
            format!("overlap / invalid layout: {error}; save refused  "),
            Style::default().fg(Color::Red),
        ));
    }
    if editor.lost_edit {
        notices.push(Span::styled(
            "A concurrent edit replaced your previous layout.  ",
            Style::default().fg(Color::Yellow),
        ));
    }
    if let Some(selected) = selected {
        for edge in edges.iter().filter(|edge| edge.from == selected.id) {
            if let Some(index) = editor.placements.iter().position(|p| p.id == edge.to) {
                let side = match edge.edge {
                    Edge::Left => "left",
                    Edge::Right => "right",
                    Edge::Top => "top",
                    Edge::Bottom => "bottom",
                    Edge::Unspecified => continue,
                };
                notices.push(Span::styled(
                    format!("{side} → {}  ", editor.names[index]),
                    Style::default().fg(Color::Green),
                ));
            }
        }
    }
    frame.render_widget(
        Paragraph::new(Line::from(notices)).wrap(ratatui::widgets::Wrap { trim: false }),
        status,
    );
    frame.render_widget(Paragraph::new("Tab / Shift+Tab: select • Arrows: move 100 • Shift+arrow: move 10\ns / Enter: save and quit • q / Esc: quit without saving"), help);
}

fn bounds(placements: &[Placement]) -> ([f64; 2], [f64; 2]) {
    const MARGIN: f64 = 100.0;
    let mut left = f64::INFINITY;
    let mut top = f64::INFINITY;
    let mut right = f64::NEG_INFINITY;
    let mut bottom = f64::NEG_INFINITY;
    for p in placements {
        left = left.min(f64::from(p.x));
        top = top.min(f64::from(p.y));
        right = right.max(f64::from(p.x) + f64::from(p.width));
        bottom = bottom.max(f64::from(p.y) + f64::from(p.height));
    }
    if placements.is_empty() {
        return ([-MARGIN, MARGIN], [-MARGIN, MARGIN]);
    }
    (
        [left - MARGIN, right + MARGIN],
        [-bottom - MARGIN, -top + MARGIN],
    )
}

fn shared_segment(from: &Placement, to: &Placement, edge: Edge) -> CanvasLine {
    let left = f64::from(from.x).max(f64::from(to.x));
    let right =
        (f64::from(from.x) + f64::from(from.width)).min(f64::from(to.x) + f64::from(to.width));
    let top = f64::from(from.y).max(f64::from(to.y));
    let bottom =
        (f64::from(from.y) + f64::from(from.height)).min(f64::from(to.y) + f64::from(to.height));
    let (x1, y1, x2, y2) = match edge {
        Edge::Left => (left, -top, left, -bottom),
        Edge::Right => (right, -top, right, -bottom),
        Edge::Top => (left, -top, right, -top),
        Edge::Bottom => (left, -bottom, right, -bottom),
        Edge::Unspecified => (left, -top, left, -top),
    };
    CanvasLine {
        x1,
        y1,
        x2,
        y2,
        color: Color::Green,
    }
}
