//! Exact one-cell zone topology repair.
//!
//! Terrain mutation owners use the explicit packed-coordinate
//! `AssignOrphaned` / `MergeAdjacent` contract and the shared hierarchy
//! patcher. Callers without that provenance rebuild the zones in full.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/zone_map, sim/zone_build, sim/zone_hierarchy.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use super::PathGrid;
use super::zone_build::hierarchy::{
    LocalHierarchyPatchResult, build_zone_hierarchy_with_query, patch_zone_hierarchy_with_query,
};
use super::zone_build::projected_zone_record_index;
use super::zone_map::{ZONE_INVALID, ZoneGrid, ZoneId};
use crate::map::resolved_terrain::{ResolvedTerrainGrid, zone_class};

#[cfg(test)]
mod native_repair_tests {
    include!("base_zone_repair_native_tests.rs");
}

/// Native `CellStruct`: signed X in the low word, signed Y in the high word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PackedZoneCoord(u32);

impl PackedZoneCoord {
    pub(crate) const fn new(x: i16, y: i16) -> Self {
        Self((x as u16 as u32) | ((y as u16 as u32) << 16))
    }

    pub(crate) const fn unpack(self) -> (i16, i16) {
        (self.0 as u16 as i16, (self.0 >> 16) as u16 as i16)
    }
}

/// Ordered586990 callbacks. Recalc owns live Cell/cache publication; this
/// controller never changes base connectivity or infers Assign/Merge repairs.
pub(crate) trait ZoneBatchHost {
    type Error;
    fn admitted(&mut self, coord: (i16, i16)) -> Result<bool, Self::Error>;
    fn clear_fine_zone(&mut self, coord: (i16, i16)) -> Result<(), Self::Error>;
    fn recalc_at(&mut self, coord: (i16, i16)) -> Result<(), Self::Error>;
    fn fine_zone(&self, coord: (i16, i16)) -> Result<ZoneId, Self::Error>;
    fn patch_hierarchy(&mut self, coord: (i16, i16)) -> Result<(), Self::Error>;
}

/// Original MapClass::RecalcCellsAndRebuildZones @0x00586990. Both passes walk
/// the supplied vector backwards, including duplicate coordinates. Admission
/// is queried again after all recalculations, and each patch observes the
/// hierarchy produced by earlier patches. Evidence: bridge_hierarchy native
/// corpus; caller-owned rectangle/span enumeration remains separate.
pub(crate) fn recalculate_zone_batch<H: ZoneBatchHost>(
    host: &mut H,
    cells: &[(i16, i16)],
) -> Result<(), H::Error> {
    for &coord in cells.iter().rev() {
        if host.admitted(coord)? {
            host.clear_fine_zone(coord)?;
            host.recalc_at(coord)?;
        }
    }
    for &coord in cells.iter().rev() {
        if host.admitted(coord)? && host.fine_zone(coord)? == ZONE_INVALID {
            host.patch_hierarchy(coord)?;
        }
    }
    Ok(())
}

/// Original5868A0 enumerates X outer/Y inner, stores each signed low word,
/// then586990 traverses that retained vector in reverse in both passes.
pub(crate) fn recalculate_zone_rectangle<H: ZoneBatchHost>(
    host: &mut H,
    [x, y, width, height]: [i32; 4],
) -> Result<(), H::Error> {
    let mut cells = Vec::new();
    for x in x..x.wrapping_add(width) {
        for y in y..y.wrapping_add(height) {
            cells.push((x as i16, y as i16));
        }
    }
    recalculate_zone_batch(host, &cells)
}

/// Provenance selects between the two distinct native one-cell helpers —
/// `MapClass::AssignOrphanedCellZone` @ `0x0056D460` and
/// `MapClass::MergeAdjacentCellZone` @ `0x0056D5A0`. It is never inferred from
/// the target's current class: gamemd's distinction is which helper the caller
/// chose, not anything readable off the cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ZoneRepairKind {
    AssignOrphaned,
    // Verified repair arm retained until its terrain mutation owner is wired.
    MergeAdjacent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ZoneRepairOutcome {
    OutsideNoOp,
    SentinelNoOp,
    Adopted { cluster: ZoneId },
    FullRebuild,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BaseRepairDecision {
    OutsideNoOp,
    SentinelNoOp,
    Adopt(ZoneId),
    FullRebuild,
}

/// Apply one verified base-cluster repair and then the shared local hierarchy
/// updater, `MapClass::IncrementalRebuildZoneGraphAroundCell` @ `0x00584550`.
///
/// Ten call sites in nine enclosing native functions invoke
/// `AssignOrphanedCellZone`/`MergeAdjacentCellZone` and then `0x00584550`
/// unconditionally 11-15 bytes later.
/// They are `AnimClass::Middle`, the area-damage helper,
/// `CellClass::DestroyOverlay`, the post-destruction wall cleanup,
/// sell-building-at-cell, `TerrainClass::Limbo`,
/// `BuildingClass::Place_OccupyMap`, `OverlayClass::Mark`, and `FUN_0074E930`
/// (call sites `0x0074EA05` / `0x0074EA10`).
///
/// Current mutation owners wire the explicit provenance in their own Phase-3
/// items; callers without it must keep using a full rebuild.
/// Executable selector/adoption evidence: tools/spatial_oracle/base_zone_repair.py;
/// hierarchy evidence: tools/spatial_oracle/bridge_hierarchy.py.
pub(crate) fn repair_zone_cell(
    zone_grid: &mut ZoneGrid,
    packed_coord: PackedZoneCoord,
    kind: ZoneRepairKind,
    path_grid: &PathGrid,
    bounds: Option<crate::map::playfield::PlayfieldBounds>,
    resolved_terrain: &ResolvedTerrainGrid,
    bridge_records: &[crate::sim::bridge_state::BridgeEndpointRecord],
) -> ZoneRepairOutcome {
    let coord = packed_coord.unpack();
    let width = zone_grid.width();
    let height = zone_grid.height();
    //56D460/56D5A0 read retained attributes only. Publication belongs to the
    //preceding Recalc, not to repair; raw Cell changes may intentionally differ.
    let base = zone_grid.base_topology_mut();
    let index = projected_zone_record_index(width, height, base.native_bridge_source_size, coord);
    let decision = if let Some(index) = index {
        decide_base_zone_repair(
            base,
            index,
            (index % usize::from(width)) as i32,
            (index / usize::from(width)) as i32,
            width,
            height,
            kind,
        )
    } else if base.native_bridge_source_size.is_some() {
        BaseRepairDecision::SentinelNoOp // Native padding has cached class7.
    } else {
        BaseRepairDecision::OutsideNoOp // No native storage in compatibility fixtures.
    };

    let outcome = match decision {
        BaseRepairDecision::OutsideNoOp => ZoneRepairOutcome::OutsideNoOp,
        BaseRepairDecision::SentinelNoOp => ZoneRepairOutcome::SentinelNoOp,
        BaseRepairDecision::Adopt(cluster) => {
            let index = index.expect("only a represented retained cell can adopt");
            zone_grid.base_topology_mut().zone_ids[index] = cluster;
            zone_grid.project_adopted_base_cell(index);
            ZoneRepairOutcome::Adopted { cluster }
        }
        BaseRepairDecision::FullRebuild => {
            zone_grid.rebuild_base_connectivity_preserving_hierarchy(
                path_grid,
                resolved_terrain,
                bridge_records,
            );
            ZoneRepairOutcome::FullRebuild
        }
    };

    // Every native caller invokes584550 independently of base eligibility.
    repair_zone_hierarchy_around_cell(zone_grid, coord, bounds, resolved_terrain, bridge_records);
    outcome
}

/// Shared hierarchy-only584550 delivery; preserves all retained base IDs/rows.
/// Rust currently compacts when itsu16 append IDs are exhausted. Native instead
/// reaches584D64 recovery on dword record-byte-offset wrap at584A8B; equivalence
/// at these storage boundaries remains open. Both world paths retain the same
/// existing Rust recovery instead of leaving a partly patched graph on capacity.
pub(crate) fn repair_zone_hierarchy_around_cell(
    zone_grid: &mut ZoneGrid,
    coord: (i16, i16),
    bounds: Option<crate::map::playfield::PlayfieldBounds>,
    resolved_terrain: &ResolvedTerrainGrid,
    bridge_records: &[crate::sim::bridge_state::BridgeEndpointRecord],
) {
    let (width, height) = (zone_grid.width(), zone_grid.height());
    let (base, hierarchy) = zone_grid.base_and_hierarchy_mut();
    // Every native caller invokes584550 independently of base eligibility.
    let patch_result = patch_zone_hierarchy_with_query(
        hierarchy,
        base,
        resolved_terrain,
        bridge_records,
        coord,
        width,
        height,
        &mut |x, y| {
            crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                (x, y),
                bounds,
                Some(resolved_terrain),
            )
        },
    );
    if patch_result == LocalHierarchyPatchResult::NeedsFullRebuild {
        let base = zone_grid.base_topology_mut().clone();
        zone_grid.replace_hierarchy(build_zone_hierarchy_with_query(
            &base,
            Some(resolved_terrain),
            bridge_records,
            width,
            height,
            &mut |x, y| {
                crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                    (x, y),
                    bounds,
                    Some(resolved_terrain),
                )
            },
        ));
    }
}

fn decide_base_zone_repair(
    base: &super::zone_build::BaseZoneTopology,
    target_index: usize,
    x: i32,
    y: i32,
    width: u16,
    height: u16,
    kind: ZoneRepairKind,
) -> BaseRepairDecision {
    let target_type = base.movement_classes[target_index];
    if target_type == zone_class::OUTSIDE {
        return BaseRepairDecision::SentinelNoOp;
    }

    let neighbors: [(u8, ZoneId); 8] = std::array::from_fn(|neighbor_index| {
        let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[neighbor_index];
        let (nx, ny) = if let Some(size) = base.native_bridge_source_size {
            // The target was clamped once. Neighbors use raw pointer offsets
            // around that canonical native record, not separately clamped coords.
            let side = size.0.wrapping_add(size.1).wrapping_add(1);
            let Some(count) = side
                .checked_mul(side)
                .filter(|&count| side > 0 && count > 0)
            else {
                return (zone_class::OUTSIDE, ZONE_INVALID);
            };
            let native = y
                .wrapping_mul(side)
                .wrapping_add(x)
                .wrapping_add(dy.wrapping_mul(side))
                .wrapping_add(dx);
            if !(0..count).contains(&native) {
                return (zone_class::OUTSIDE, ZONE_INVALID);
            }
            (native % side, native / side)
        } else {
            (x + dx, y + dy)
        };
        if nx < 0 || ny < 0 || nx >= i32::from(width) || ny >= i32::from(height) {
            return (zone_class::OUTSIDE, ZONE_INVALID);
        }
        let index = ny as usize * width as usize + nx as usize;
        (
            base.movement_classes[index],
            base.zone_ids.get(index).copied().unwrap_or(ZONE_INVALID),
        )
    });

    let candidate = neighbors.iter().find(|&&(neighbor_type, _)| match kind {
        ZoneRepairKind::AssignOrphaned => neighbor_type == zone_class::GROUND,
        ZoneRepairKind::MergeAdjacent => neighbor_type == target_type,
    });
    let Some(&(_, candidate_cluster)) = candidate else {
        return BaseRepairDecision::FullRebuild;
    };

    let row0 = &base.raw_zone_ids_by_row[0];
    // Native reads the retained row even for cluster0.56CAF4 normally stores
    // FFFF there, but the selectors themselves do not substitute the sentinel.
    let mapped = |cluster: ZoneId| row0.get(cluster as usize).copied().unwrap_or(u16::MAX);
    let mut previous_cluster = ZONE_INVALID;
    let mut transitions = 0u8;
    for &(neighbor_type, cluster) in &neighbors {
        if mapped(cluster) != mapped(previous_cluster) && neighbor_type != zone_class::OUTSIDE {
            transitions += 1;
            previous_cluster = cluster;
        }
    }
    if transitions >= 4
        || (kind == ZoneRepairKind::AssignOrphaned && target_type != zone_class::GROUND)
    {
        BaseRepairDecision::FullRebuild
    } else {
        BaseRepairDecision::Adopt(candidate_cluster)
    }
}
