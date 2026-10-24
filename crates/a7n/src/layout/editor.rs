// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::{LayoutInfo, LayoutPlacement, Placement, validate};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

const MOVE_STEP: i32 = 100;
const FINE_MOVE_STEP: i32 = 10;

#[derive(Debug)]
pub(super) struct Editor {
    pub(super) placements: Vec<Placement>,
    pub(super) names: Vec<String>,
    pub(super) selected: usize,
    pub(super) lost_edit: bool,
}

#[derive(Debug)]
pub(super) enum Action {
    Continue,
    Quit,
    Save(Vec<LayoutPlacement>),
}

impl Editor {
    pub(super) fn new(info: LayoutInfo) -> Result<Self> {
        let draft = a7n_ipc::layout::LayoutDraft::try_from(info)?;
        Ok(Self {
            placements: draft.placements,
            names: draft.names,
            selected: 0,
            lost_edit: draft.lost_edit,
        })
    }

    pub(super) fn handle(&mut self, key: KeyEvent) -> Action {
        if key.kind == KeyEventKind::Release {
            return Action::Continue;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => return Action::Quit,
            KeyCode::Enter | KeyCode::Char('s') if validate(&self.placements).is_ok() => {
                return Action::Save(
                    self.placements
                        .iter()
                        .map(|p| LayoutPlacement {
                            machine: p.id,
                            x: p.x,
                            y: p.y,
                            width: p.width,
                            height: p.height,
                        })
                        .collect(),
                );
            }
            _ => {}
        }
        if self.placements.is_empty() {
            return Action::Continue;
        }
        let step = if key.modifiers.contains(KeyModifiers::SHIFT) {
            FINE_MOVE_STEP
        } else {
            MOVE_STEP
        };
        let placement = &mut self.placements[self.selected];
        match key.code {
            KeyCode::BackTab => self.select_previous(),
            KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => self.select_previous(),
            KeyCode::Tab => self.selected = (self.selected + 1) % self.placements.len(),
            KeyCode::Left => placement.x = placement.x.saturating_sub(step),
            KeyCode::Right => placement.x = placement.x.saturating_add(step),
            KeyCode::Up => placement.y = placement.y.saturating_sub(step),
            KeyCode::Down => placement.y = placement.y.saturating_add(step),
            _ => {}
        }
        Action::Continue
    }

    fn select_previous(&mut self) {
        self.selected = (self.selected + self.placements.len() - 1) % self.placements.len();
    }
}

#[cfg(test)]
mod tests;
