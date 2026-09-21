//! Native bridge-pavement damage-selector flood fill.
//!
//! This is sim-owned CellClass state. Presentation consumes the returned
//! ordered cells through the generic radar-terrain dirty channel.

use super::BridgeRuntimeState;
#[cfg(test)]
use crate::map::resolved_terrain::ResolvedTerrainGrid;

pub(super) fn extend_unique_cells(
    target: &mut Vec<(u16, u16)>,
    cells: impl IntoIterator<Item = (u16, u16)>,
) {
    for cell in cells {
        if !target.contains(&cell) {
            target.push(cell);
        }
    }
}

impl BridgeRuntimeState {
    /// Compatibility call surface for existing bridge drivers. The state is
    /// owned exclusively by live terrain, including cells without a bridge entry.
    /// Callers retain returned real-cell writes before dispatching other effects.
    #[cfg(test)]
    pub fn apply_damaged_variant_flood_fill(
        &mut self,
        rx: u16,
        ry: u16,
        state: bool,
        terrain: &mut ResolvedTerrainGrid,
    ) -> Vec<(u16, u16)> {
        terrain.apply_native_pavement((rx as i16, ry as i16), state)
    }
}
