// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure layout draft initialization shared by graphical and terminal editors.
use crate::{LayoutInfo, LayoutPlacement, Placement};

const DEFAULT_MACHINE_SIZE: (u32, u32) = (1920, 1080);

#[derive(Debug)]
pub struct LayoutDraft {
    pub placements: Vec<Placement>,
    pub names: Vec<String>,
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
            let x = i32::try_from(right).map_err(|_| LayoutDraftError)?;
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
            machine: MachineId::try_from([id; MachineId::LEN].as_slice()).unwrap(),
            name: format!("machine-{id}"),
            x,
            y,
            width: DEFAULT_MACHINE_SIZE.0,
            height: DEFAULT_MACHINE_SIZE.1,
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
                    machine: MachineId::try_from([id; MachineId::LEN].as_slice()).unwrap(),
                    name: format!("machine-{id}"),
                })
                .collect(),
            lost_edit: false,
        };

        // Act
        let editor = LayoutDraft::try_from(info).unwrap();

        // Assert
        assert_eq!(editor.placements[2].x, 1920);
        assert_eq!(editor.placements[3].x, 3840);
        assert_eq!(editor.placements[2].y, -100);
        assert_eq!(editor.placements[3].y, -100);
        assert!(validate(&editor.placements).is_ok());
    }
}
