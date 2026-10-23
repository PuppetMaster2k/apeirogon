// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_core::layout::{Layout, Merge, Placement};
use a7n_ipc::{LayoutInfo, LayoutPlacement, PlacedMachine, UnplacedMachine};
use a7n_proto::{
    MachineId,
    v1::{self, Envelope, envelope::Body},
};

use super::{Daemon, DaemonError};

impl Daemon {
    pub(super) fn layout_info(&self) -> LayoutInfo {
        let placed = self
            .layout
            .state
            .placements()
            .iter()
            .map(|p| PlacedMachine {
                machine: p.id,
                name: self.layout_name(p.id),
                x: p.x,
                y: p.y,
                width: p.width,
                height: p.height,
            })
            .collect();
        let unplaced = std::iter::once(self.identity.machine_id)
            .chain(
                self.pins
                    .machine_ids()
                    .filter(|id| *id != self.identity.machine_id),
            )
            .filter(|id| !self.layout.state.placements().iter().any(|p| p.id == *id))
            .map(|machine| UnplacedMachine {
                machine,
                name: self.layout_name(machine),
            })
            .collect();
        LayoutInfo {
            placed,
            unplaced,
            lost_edit: self.layout.lost_edit,
        }
    }

    fn layout_name(&self, id: MachineId) -> String {
        if id == self.identity.machine_id {
            return self.info.name.clone();
        }
        self.pins
            .get(&id)
            .map_or_else(|| "unknown".into(), |entry| entry.name.clone())
    }

    pub(super) fn set_layout(&mut self, placed: Vec<LayoutPlacement>) -> Result<(), DaemonError> {
        let placements = placed
            .into_iter()
            .map(|p| Placement {
                id: p.machine,
                x: p.x,
                y: p.y,
                width: p.width,
                height: p.height,
            })
            .collect();
        let edited = self
            .layout
            .state
            .edit(placements, self.identity.machine_id)?;
        self.layout.persist(edited)?;
        self.layout.lost_edit = false;
        self.broadcast(&Envelope {
            body: Some(Body::Layout(self.layout.state.to_proto())),
        });
        Ok(())
    }

    pub(super) fn receive_layout(&mut self, wire: &v1::Layout) -> Result<(), DaemonError> {
        let Ok(incoming) = Layout::try_from(wire) else {
            crate::log::event!("kind=layout_rejected");
            return Ok(());
        };
        let mut staged = self.layout.state.clone();
        if let Merge::Replaced { lost_own_edit } = staged.merge(incoming, self.identity.machine_id)
        {
            self.layout.persist(staged)?;
            if lost_own_edit {
                crate::log::event!("kind=layout_edit_lost");
                self.layout.lost_edit = true;
            }
            self.broadcast(&Envelope {
                body: Some(Body::Layout(self.layout.state.to_proto())),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
