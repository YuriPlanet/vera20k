//! Repair-only hierarchy publication: original5851B0 followed by56D100.
//! Native corpus: tools/spatial_oracle/bridge_repair_zones.{py,json,meta.json}.
//! Full/local582D70 temporary edge buckets have different duplicate semantics.

use super::{ZoneGrid, cell_is_in_native_map_diamond};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::MovementZone;
use crate::sim::bridge_state::BridgeEndpointRecord;
use crate::sim::cell_rect::{
    CellRef, PlayfieldBounds, cell_is_in_playfield_height_aware, get_cellclass_fallback,
};
use crate::sim::pathfinding::zone_build::hierarchy::HIGH_BRIDGE_HIERARCHY_DIRECTIONS;
use crate::sim::pathfinding::zone_hierarchy::{ZoneEdgeRecord, ZoneHierarchy};

/// Original5851B0 directly appends six directed edges per level. Preserve
/// existing entries, self edges, node0, duplicates, pair order and flag byte0.
/// Each word lookup uses the requested packed endpoint, not its Cell allocation.
pub(crate) fn append_repaired_bridge_edges(
    hierarchy: &mut ZoneHierarchy,
    record: &BridgeEndpointRecord,
    direction: u8,
    source_size: Option<(i32, i32)>,
) -> Result<(), String> {
    let step = |coord: (u16, u16), direction: u8| {
        let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[usize::from(direction & 7)];
        (
            coord.0.wrapping_add(dx as u16),
            coord.1.wrapping_add(dy as u16),
        )
    };
    let (a, b) = (record.endpoint_a, record.endpoint_b);
    let pairs = [
        (a, b),
        (step(a, direction), step(b, direction)),
        (
            step(a, direction.wrapping_sub(4)),
            step(b, direction.wrapping_sub(4)),
        ),
    ];
    for graph in hierarchy.levels_mut() {
        for (from, to) in pairs {
            let from = graph
                .native_zone_at(from, source_size)
                .ok_or("repair bridge source has no native hierarchy projection")?;
            let to = graph
                .native_zone_at(to, source_size)
                .ok_or("repair bridge destination has no native hierarchy projection")?;
            for (from, to) in [(from, to), (to, from)] {
                // Native sign-extends this word into an unchecked node pointer.
                // Retain prior writes and expose unsupported pointer inputs.
                if (!graph.cell_grid().is_extended() && from > i16::MAX as super::ZoneId)
                    || from as usize >= graph.record_slot_count()
                {
                    return Err(format!(
                        "repair bridge hierarchy node {from} is not represented"
                    ));
                }
                graph.push_edge(from, ZoneEdgeRecord::new(to, 0));
            }
        }
    }
    Ok(())
}

impl ZoneGrid {
    /// Called after the record's native active byte is set. The return value
    /// requests the separate56C510 base connectivity pass; this operation never
    /// changes base cluster IDs or any of the13 raw movement rows.
    pub(crate) fn activate_repaired_bridge(
        &mut self,
        terrain: &ResolvedTerrainGrid,
        record: &BridgeEndpointRecord,
        bounds: Option<PlayfieldBounds>,
    ) -> Result<bool, String> {
        self.invalidate_navigation_history();
        let signed = |p: (u16, u16)| (i32::from(p.0 as i16), i32::from(p.1 as i16));
        let a = signed(record.endpoint_a);
        let b = signed(record.endpoint_b);
        let offset = match get_cellclass_fallback(Some(terrain), a.0, a.1) {
            CellRef::Real(cell) => terrain.high_bridge_tile_offset(cell),
            CellRef::Dummy { .. } => None,
        }
        .ok_or("repair bridge endpoint A is outside the proven high-tile domain")?;
        let size = self
            .native_bridge_source_size
            .ok_or("repair bridge has no native Map Size")?;
        append_repaired_bridge_edges(
            &mut self.hierarchy,
            record,
            HIGH_BRIDGE_HIERARCHY_DIRECTIONS[offset] as u8,
            Some(size),
        )?;
        //56D100 mode1 A lookup, optional A-fringe shortcut, then mode1 B
        // lookup even though B-fringe is disabled. The B lookup can stamp the
        // shared dummy. Raw zone reads follow B then A; equal0/1/FFFF succeed.
        if !cell_is_in_playfield_height_aware(a, bounds, Some(terrain))
            && cell_is_in_native_map_diamond(a, size.0, size.1)
        {
            return Ok(false);
        }
        let _ = cell_is_in_playfield_height_aware(b, bounds, Some(terrain));
        let zb = self
            .get_zone_id_nonbridge_native(b, MovementZone::Normal)
            .ok_or("repair bridge destination row0 is not represented")?;
        let za = self
            .get_zone_id_nonbridge_native(a, MovementZone::Normal)
            .ok_or("repair bridge source row0 is not represented")?;
        Ok(za != zb)
    }

    /// Synchronize the retained source records after native record mutation.
    /// Replacing the hierarchy here would erase5851B0's ordered append history.
    pub(crate) fn retain_repaired_bridge_records(
        &mut self,
        path: &crate::sim::pathfinding::PathGrid,
        terrain: &ResolvedTerrainGrid,
        records: &[BridgeEndpointRecord],
    ) {
        self.invalidate_navigation_history();
        // Compatibility readers retain a derived redirect per movement row.
        // Activation changes structural-cell lookup from the broken span's
        // chosen endpoint to A even when56DB70 requests no connectivity pass.
        // This legacy projection is side-effect free (no native dummy writes).
        let redirect = crate::sim::pathfinding::zone_build::build_bridge_redirect(
            path,
            Some(terrain),
            records,
            self.width,
            self.height,
        );
        for (movement, map) in &mut self.maps {
            if movement.can_use_bridges() {
                map.set_bridge_redirect(redirect.clone());
            }
        }
        self.bridge_records = records.to_vec();
    }
}

#[cfg(all(test, any(tests_sim_pathfinding, tests_sim_movement)))]
#[path = "bridge_repair_zones_tests.rs"]
mod tests;
