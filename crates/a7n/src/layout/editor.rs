// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::{LayoutInfo, LayoutPlacement, Placement, validate};
use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

const DEFAULT_MACHINE_SIZE: (u32, u32) = (1920, 1080);
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
        let mut placements: Vec<_> = info
            .placed
            .iter()
            .map(|machine| Placement {
                id: machine.machine,
                x: machine.x,
                y: machine.y,
                width: machine.width,
                height: machine.height,
            })
            .collect();
        let mut right = placements
            .iter()
            .map(|p| i64::from(p.x) + i64::from(p.width))
            .max()
            .unwrap_or(0);
        let top = placements.iter().map(|p| p.y).min().unwrap_or(0);
        let mut names: Vec<_> = info
            .placed
            .into_iter()
            .map(|machine| machine.name)
            .collect();
        for machine in info.unplaced {
            let x = i32::try_from(right).context("no canvas space to place unplaced machines")?;
            placements.push(Placement {
                id: machine.machine,
                x,
                y: top,
                width: DEFAULT_MACHINE_SIZE.0,
                height: DEFAULT_MACHINE_SIZE.1,
            });
            names.push(machine.name);
            right += i64::from(DEFAULT_MACHINE_SIZE.0);
        }
        Ok(Self {
            placements,
            names,
            selected: 0,
            lost_edit: info.lost_edit,
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
