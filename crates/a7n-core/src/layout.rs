// SPDX-License-Identifier: GPL-3.0-or-later

//! Validated desk canvas and last-writer-wins layout merge (PROTOCOL.md 7).

pub use a7n_proto::v1::Edge;
use a7n_proto::{MachineId, ProtoError, limits, v1};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub id: MachineId,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error(transparent)]
    InvalidId(#[from] ProtoError),
    #[error("layout exceeds the machine limit")]
    TooManyMachines,
    #[error("machine rectangle has zero size")]
    ZeroSize,
    #[error("layout contains a duplicate machine id")]
    DuplicateId,
    #[error("machine rectangle exceeds the canvas coordinate range")]
    CoordinateOverflow,
    #[error("machine rectangles overlap")]
    Overlap,
    #[error("layout version is exhausted")]
    VersionExhausted,
}

/// Validate all placements before storing or sharing a canvas.
///
/// # Errors
/// Rejects excessive, duplicate, empty, overflowing or overlapping rectangles.
pub fn validate(placements: &[Placement]) -> Result<(), LayoutError> {
    if placements.len() > limits::LAYOUT_MACHINES_MAX {
        return Err(LayoutError::TooManyMachines);
    }
    for (index, p) in placements.iter().enumerate() {
        if p.width == 0 || p.height == 0 {
            return Err(LayoutError::ZeroSize);
        }
        if right(p) > i64::from(i32::MAX) || bottom(p) > i64::from(i32::MAX) {
            return Err(LayoutError::CoordinateOverflow);
        }
        for other in &placements[..index] {
            if p.id == other.id {
                return Err(LayoutError::DuplicateId);
            }
            if i64::from(p.x) < right(other)
                && i64::from(other.x) < right(p)
                && i64::from(p.y) < bottom(other)
                && i64::from(other.y) < bottom(p)
            {
                return Err(LayoutError::Overlap);
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    version: u64,
    by: MachineId,
    placements: Vec<Placement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Merge {
    Unchanged,
    Replaced { lost_own_edit: bool },
}

impl Layout {
    #[must_use]
    pub fn empty(own_id: MachineId) -> Self {
        Self {
            version: 0,
            by: own_id,
            placements: Vec::new(),
        }
    }

    #[must_use]
    pub fn version(&self) -> u64 {
        self.version
    }

    #[must_use]
    pub fn by(&self) -> MachineId {
        self.by
    }

    #[must_use]
    pub fn placements(&self) -> &[Placement] {
        &self.placements
    }

    /// One-based machine number, ordered top to bottom, then left to right.
    #[must_use]
    pub fn nth(&self, n: u8) -> Option<MachineId> {
        let index = usize::from(n.checked_sub(1)?);
        let mut placements: Vec<_> = self.placements.iter().collect();
        placements.sort_unstable_by_key(|p| (p.y, p.x));
        placements.get(index).map(|p| p.id)
    }

    /// # Errors
    /// Rejects invalid placements and an exhausted version counter.
    pub fn edit(&self, placements: Vec<Placement>, own_id: MachineId) -> Result<Self, LayoutError> {
        validate(&placements)?;
        // ponytail: versions stop at u64::MAX; change the protocol before exhausting it.
        let version = self
            .version
            .checked_add(1)
            .ok_or(LayoutError::VersionExhausted)?;
        Ok(Self {
            version,
            by: own_id,
            placements,
        })
    }

    pub fn merge(&mut self, incoming: Self, own_id: MachineId) -> Merge {
        if (incoming.version, incoming.by) <= (self.version, self.by) {
            return Merge::Unchanged;
        }
        let lost_own_edit = self.by == own_id && self.version == incoming.version;
        *self = incoming;
        Merge::Replaced { lost_own_edit }
    }

    #[must_use]
    pub fn to_proto(&self) -> v1::Layout {
        v1::Layout {
            version: self.version,
            by: self.by.as_bytes().to_vec(),
            machines: self
                .placements
                .iter()
                .map(|p| v1::MachineRect {
                    id: p.id.as_bytes().to_vec(),
                    x: p.x,
                    y: p.y,
                    width: p.width,
                    height: p.height,
                })
                .collect(),
        }
    }

    /// Map an exit to a touching machine's opposite edge and fractional offset.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // Landing uses f32 on the wire; geometry uses f64.
    pub fn exit(
        &self,
        from: MachineId,
        edge: Edge,
        along: f32,
    ) -> Option<(MachineId, v1::Landing)> {
        if along.is_nan() {
            return None;
        }
        let source = self.placements.iter().find(|p| p.id == from)?;
        let horizontal = matches!(edge, Edge::Top | Edge::Bottom);
        let (start, size) = if horizontal {
            (source.x, source.width)
        } else {
            (source.y, source.height)
        };
        let point = f64::from(start) + f64::from(size) * f64::from(along.clamp(0.0, 1.0));
        for target in &self.placements {
            if target.id == from || !touches(source, target, edge) {
                continue;
            }
            let (start, size) = if horizontal {
                (target.x, target.width)
            } else {
                (target.y, target.height)
            };
            let offset = (point - f64::from(start)) / f64::from(size);
            if (0.0..1.0).contains(&offset) {
                return Some((
                    target.id,
                    v1::Landing {
                        edge: opposite(edge) as i32,
                        offset: offset as f32,
                        ..v1::Landing::default()
                    },
                ));
            }
        }
        None
    }
}

impl TryFrom<&v1::Layout> for Layout {
    type Error = LayoutError;

    fn try_from(value: &v1::Layout) -> Result<Self, Self::Error> {
        if value.machines.len() > limits::LAYOUT_MACHINES_MAX {
            return Err(LayoutError::TooManyMachines);
        }
        let by = MachineId::try_from(value.by.as_slice())?;
        let placements = value
            .machines
            .iter()
            .map(|p| {
                Ok(Placement {
                    id: MachineId::try_from(p.id.as_slice())?,
                    x: p.x,
                    y: p.y,
                    width: p.width,
                    height: p.height,
                })
            })
            .collect::<Result<Vec<_>, LayoutError>>()?;
        validate(&placements)?;
        Ok(Self {
            version: value.version,
            by,
            placements,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SharedEdge {
    pub from: MachineId,
    pub to: MachineId,
    pub edge: Edge,
}

#[must_use]
pub fn shared_edges(placements: &[Placement]) -> Vec<SharedEdge> {
    let mut edges = Vec::new();
    for from in placements {
        for to in placements {
            if from.id == to.id {
                continue;
            }
            for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
                let overlap = if matches!(edge, Edge::Left | Edge::Right) {
                    i64::from(from.y).max(i64::from(to.y)) < bottom(from).min(bottom(to))
                } else {
                    i64::from(from.x).max(i64::from(to.x)) < right(from).min(right(to))
                };
                if overlap && touches(from, to, edge) {
                    edges.push(SharedEdge {
                        from: from.id,
                        to: to.id,
                        edge,
                    });
                }
            }
        }
    }
    edges
}

fn right(p: &Placement) -> i64 {
    i64::from(p.x) + i64::from(p.width)
}
fn bottom(p: &Placement) -> i64 {
    i64::from(p.y) + i64::from(p.height)
}

fn touches(from: &Placement, to: &Placement, edge: Edge) -> bool {
    match edge {
        Edge::Left => i64::from(from.x) == right(to),
        Edge::Right => right(from) == i64::from(to.x),
        Edge::Top => i64::from(from.y) == bottom(to),
        Edge::Bottom => bottom(from) == i64::from(to.y),
        Edge::Unspecified => false,
    }
}

fn opposite(edge: Edge) -> Edge {
    match edge {
        Edge::Left => Edge::Right,
        Edge::Right => Edge::Left,
        Edge::Top => Edge::Bottom,
        Edge::Bottom => Edge::Top,
        Edge::Unspecified => Edge::Unspecified,
    }
}
