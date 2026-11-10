// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure layout draft initialization shared by graphical and terminal editors.
use crate::{LayoutInfo, LayoutPlacement, Placement};

/// Canvas units per millimeter of physical desktop size.
pub const UNITS_PER_MM: u32 = 10;

/// Convert physical dimensions to canvas units without overflowing.
#[must_use]
pub fn screen_size_units((width, height): (u32, u32)) -> (u32, u32) {
    (
        width.saturating_mul(UNITS_PER_MM),
        height.saturating_mul(UNITS_PER_MM),
    )
}

const DEFAULT_MACHINE_SIZE: (u32, u32) = (1920, 1080);

#[derive(Debug)]
pub struct LayoutDraft {
    pub placements: Vec<Placement>,
    pub names: Vec<String>,
    pub screen_mm: Vec<Option<(u32, u32)>>,
    pub lost_edit: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("no canvas space to place unplaced machines")]
pub struct LayoutDraftError;

impl TryFrom<LayoutInfo> for LayoutDraft {
    type Error = LayoutDraftError;

    fn try_from(info: LayoutInfo) -> Result<Self, Self::Error> {
        let mut placements: Vec<_> = info
            .placed
            .iter()
            .map(|machine| Placement {
                id: machine.machine,
                x: machine.x,
                y: machine.y,
                width: machine.width,
                height: machine.height,
                blocked: machine.blocked,
            })
            .collect();
        let mut right = placements
            .iter()
            .map(|p| i64::from(p.x) + i64::from(p.width))
            .max()
            .unwrap_or(0);
        let top = placements.iter().map(|p| p.y).min().unwrap_or(0);
        let mut screen_mm: Vec<_> = info
            .placed
            .iter()
            .map(|machine| machine.screen_mm)
            .collect();
        let mut names: Vec<_> = info
            .placed
            .into_iter()
            .map(|machine| machine.name)
            .collect();
        for machine in info.unplaced {
            let x = i32::try_from(right).map_err(|_| LayoutDraftError)?;
            let (width, height) = machine
                .screen_mm
                .map_or(DEFAULT_MACHINE_SIZE, screen_size_units);
            screen_mm.push(machine.screen_mm);
            placements.push(Placement {
                id: machine.machine,
                x,
                y: top,
                width,
                height,
                blocked: 0,
            });
            names.push(machine.name);
            right += i64::from(width);
        }
        Ok(Self {
            placements,
            names,
            screen_mm,
            lost_edit: info.lost_edit,
        })
    }
}

impl LayoutDraft {
    #[must_use]
    pub fn placed(&self) -> Vec<LayoutPlacement> {
        self.placements
            .iter()
            .map(|p| LayoutPlacement {
                machine: p.id,
                x: p.x,
                y: p.y,
                width: p.width,
                height: p.height,
                blocked: p.blocked,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MachineId, PlacedMachine, UnplacedMachine, validate};

    fn machine(id: u8, x: i32, y: i32) -> PlacedMachine {
        PlacedMachine {
            screen_mm: None,
            machine: MachineId::try_from([id; MachineId::LEN].as_slice()).unwrap(),
            name: format!("machine-{id}"),
            x,
            y,
            width: DEFAULT_MACHINE_SIZE.0,
            height: DEFAULT_MACHINE_SIZE.1,
            blocked: 0x123,
        }
    }
    #[test]
    fn try_from_unplaced_machines_places_to_right_top_aligned() {
        // Arrange
        let info = LayoutInfo {
            placed: vec![machine(1, -1920, -100), machine(2, 0, 100)],
            unplaced: [3, 4]
                .into_iter()
                .map(|id| UnplacedMachine {
                    screen_mm: None,
                    machine: MachineId::try_from([id; MachineId::LEN].as_slice()).unwrap(),
                    name: format!("machine-{id}"),
                })
                .collect(),
            lost_edit: false,
        };

        // Act
        let editor = LayoutDraft::try_from(info).unwrap();

        // Assert
        assert_eq!(editor.placements[0].blocked, 0x123);
        assert_eq!(editor.placed()[0].blocked, 0x123);
        assert_eq!(editor.placements[2].blocked, 0);
        assert_eq!(editor.placements[2].x, 1920);
        assert_eq!(editor.placements[3].x, 3840);
        assert_eq!(editor.placements[2].y, -100);
        assert_eq!(editor.placements[3].y, -100);
        assert!(validate(&editor.placements).is_ok());
    }
}
