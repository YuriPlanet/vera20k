//! 3×3 cell grid iteration helper for area-of-effect superweapons.
//!
//! Shared native cell lookup; each launch policy owns its receiver traversal.
//!
//! ## Dependency rules
//! - Reads the map-owned CellClass lookup and world-owned cell membership.

use crate::sim::cell_rect::{CellRef, get_cellclass_fallback};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::Simulation;

/// Runtime-initialized table at 0x00B0C038..0x00B0C05C. Native initializer
/// 0x006CAE00..0x006CAEBF; SuperClass::Launch adds each component as a word at
/// 0x006CCF39..0x006CCF44 (IC) and uses the same table for per-cell mutation.
/// Wrap each 16-bit component independently; never clamp an edge onto cell zero.
pub(super) fn native_cells_3x3(rx: u16, ry: u16) -> impl Iterator<Item = (i16, i16)> {
    const OFFSETS: [(i16, i16); 9] = [
        (0, 0),
        (1, 0),
        (1, -1),
        (0, -1),
        (-1, -1),
        (-1, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
    ];
    OFFSETS
        .into_iter()
        .map(move |(dx, dy)| ((rx as i16).wrapping_add(dx), (ry as i16).wrapping_add(dy)))
}

/// Select the canonical real cell and one live native object-list layer.
/// SuperClass::Launch tests CellClass+0x140 & 0x100, then reads +0xE8 or +0xE4
/// (IC 0x006CCF6A..0x006CCFEE). The existing GetCell facade owns fixed-stride
/// aliases, allocation and dummy stamping; requested coordinates are not keys.
/// The shared dummy has no represented object-list members in the Rust substrate.
pub(crate) fn selected_cell_list(
    sim: &Simulation,
    x: i16,
    y: i16,
) -> Option<((u16, u16), MovementLayer)> {
    match get_cellclass_fallback(sim.resolved_terrain.as_ref(), i32::from(x), i32::from(y)) {
        CellRef::Real(cell) => Some((
            (cell.rx, cell.ry),
            if cell.bridge_facts.raw_flags & crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL != 0 {
                MovementLayer::Bridge
            } else {
                MovementLayer::Ground
            },
        )),
        CellRef::Dummy { .. } => {
            if sim.resolved_terrain.is_none() {
                sim.effective_shared_cell_dummy()
                    .stamp_coord(i32::from(x), i32::from(y));
            }
            None
        }
    }
}

/// Whether `MapClass::GetCellAt @ 0x005657A0`'s cell for `(x, y)` carries
/// the bridge flag (`CellClass+0x140 & 0x100`). The shared dummy an off-map
/// lookup returns carries none.
pub(super) fn cell_has_bridge_flag(sim: &Simulation, x: i16, y: i16) -> bool {
    match get_cellclass_fallback(sim.resolved_terrain.as_ref(), i32::from(x), i32::from(y)) {
        CellRef::Real(cell) => {
            cell.bridge_facts.raw_flags & crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL != 0
        }
        CellRef::Dummy { .. } => false,
    }
}

/// Read an IC receiver's then-live link, including a callback that relayered
/// or relocated it. Retain the selected footprint cell while the object remains
/// on that list; after remove/re-add, its new membership owns the successor.
/// Native IC reads Object+0x30 after the call (0x006CD025); RemoveContent clears
/// it (0x0047EAF0), while PutContent may populate it from a different list.
pub(super) fn live_successor(
    sim: &Simulation,
    current: u64,
    visited_cell: (u16, u16),
    visited_layer: MovementLayer,
) -> Option<u64> {
    if let Some(cell) = sim.substrate.occupancy.get(visited_cell.0, visited_cell.1)
        && cell
            .iter_layer(visited_layer)
            .any(|member| member.entity_id == current)
    {
        return cell.next_on_layer(visited_layer, current);
    }
    let object = sim.substrate.entities.get(current)?;
    let cell = sim
        .substrate
        .occupancy
        .get(object.position.rx, object.position.ry)?;
    // PUT may retain a Cell list even though the current virtual layer query
    // differs. Read the actual replacement list, not a fresh +78 projection.
    let layer = cell
        .occupants
        .iter()
        .find(|member| member.entity_id == current)?
        .layer;
    cell.next_on_layer(layer, current)
}
