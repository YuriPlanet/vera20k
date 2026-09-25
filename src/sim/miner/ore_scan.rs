//! The Foot tiberium search Mission_Harvest runs for a harvester:
//! `FootClass::Search_For_Tiberium_And_Move @ 0x004DCFE0`,
//! `FootClass::Scan_For_Tiberium @ 0x004DD0A0` (Unit vtable `+0x338`) and
//! `FootClass::Is_Cell_Harvestable @ 0x004DCE80`.
//!
//! Evidence: `tools/spatial_oracle/harvest_field.json` `scan` and `search`
//! rows, replayed by `world/harvest_field_oracle_tests.rs`.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/world, sim/movement, sim/pathfinding,
//!   sim/tiberium, rules/.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::locomotor_type::MovementZone;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::LandType;
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;

/// A `[General]` scan radius in leptons as Mission_Harvest hands it on:
/// `CDQ; AND EDX,0xFF; ADD EAX,EDX; SAR EAX,8`, the signed division by 256
/// (`0x0073E851`, `0x0073EAA6`).
pub(super) fn scan_cells(leptons: i32) -> i32 {
    leptons.wrapping_add((leptons >> 31) & 0xFF) >> 8
}

/// Whether `cell` holds LandType 5 (`CellClass+0xEC`), the Tiberium land an
/// ore or gem overlay gives its cell. The resolved terrain's land type is the
/// authority; a fixture with no resolved terrain reads the overlay's own
/// `Land=`, the value `CellClass::RecalcAttributes @ 0x0047D2B0` writes for a
/// tiberium overlay (`NoUseTileLandType` is set).
pub(super) fn cell_is_tiberium_land(
    sim: &Simulation,
    overlay_registry: Option<&OverlayTypeRegistry>,
    cell: (u16, u16),
) -> bool {
    if let Some(terrain) = sim.resolved_terrain.as_ref() {
        return terrain
            .cell(cell.0, cell.1)
            .is_some_and(|cell| cell.land_type == LandType::Tiberium.as_index());
    }
    let overlay = sim
        .overlay_grid
        .as_ref()
        .filter(|grid| cell.0 < grid.width() && cell.1 < grid.height())
        .and_then(|grid| grid.cell(cell.0, cell.1).overlay_id);
    overlay
        .zip(overlay_registry)
        .and_then(|(id, registry)| registry.flags(id))
        .is_some_and(|flags| flags.land == LandType::Tiberium)
}

/// `CellClass::Get_Tiberium_Value @ 0x00485020`: the overlay's tiberium
/// `Value=` times `OverlayData + 1`, or 0 without a tiberium overlay.
fn tiberium_value(
    sim: &Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    cell: (u16, u16),
) -> i32 {
    match (sim.overlay_grid.as_ref(), overlay_registry) {
        (Some(grid), Some(registry)) => {
            crate::sim::tiberium::tiberium_cell_view(grid, registry, &rules.tiberium_types, cell)
                .map_or(0, |view| view.nominal_value)
        }
        _ => 0,
    }
}

/// `FootClass::Search_For_Tiberium_And_Move @ 0x004DCFE0`. A mover holding a
/// NavCom answers false at once (`0x004DCFE7`). Otherwise it scans; a hit on
/// its own cell answers true, any other hit becomes the destination through
/// the class setter `vt+0x480(cell, 1)` (`0x004DD086`) and answers false, as
/// does a miss.
pub(crate) fn search_for_tiberium_and_move(
    sim: &mut Simulation,
    rules: &RuleSet,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&OverlayTypeRegistry>,
    id: u64,
    range: i32,
) -> bool {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    if entity.navigation.nav_com.is_some() {
        return false;
    }
    let own = (entity.position.rx, entity.position.ry);
    let Some(cell) = scan_for_tiberium(sim, rules, overlay_registry, id, range) else {
        return false;
    };
    if cell == own {
        return true;
    }
    if let Some(grid) = path_grid {
        let _ = super::miner_system::issue_stock_miner_drive_move_with_overlay_registry(
            sim,
            rules,
            grid,
            id,
            cell,
            overlay_registry,
        );
    }
    false
}

/// `FootClass::Scan_For_Tiberium @ 0x004DD0A0`: the mover's own cell when
/// it is Tiberium land; else rings `r = 1..range` (the bound itself is not
/// scanned), each walking `i = -r..=r` over `(x+i, y-r)`, `(x+i, y+r)`,
/// `(x-r, y+i)`, `(x+r, y+i)` (corners twice). A harvestable cell whose
/// value beats the best so far (strictly, so ties keep the earlier cell)
/// becomes the best, and the first ring with a hit ends the scan. `None` is
/// the `(0,0)` no-cell answer (`0x008B3D88`).
pub(crate) fn scan_for_tiberium(
    sim: &Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    id: u64,
    range: i32,
) -> Option<(u16, u16)> {
    let entity = sim.substrate.entities.get(id)?;
    let own = (entity.position.rx, entity.position.ry);
    if cell_is_tiberium_land(sim, overlay_registry, own) {
        return Some(own);
    }
    let (x, y) = (i32::from(own.0), i32::from(own.1));
    let mut best_value = -1;
    let mut best = None;
    for r in 1..range {
        for i in -r..=r {
            for candidate in [
                (x + i, y - r),
                (x + i, y + r),
                (x - r, y + i),
                (x + r, y + i),
            ] {
                let Some(cell) =
                    is_cell_harvestable(sim, rules, overlay_registry, id, own, candidate)
                else {
                    continue;
                };
                let value = tiberium_value(sim, rules, overlay_registry, cell);
                if value > best_value {
                    best_value = value;
                    best = Some(cell);
                }
            }
        }
        if best_value != -1 {
            break;
        }
    }
    best
}

/// `FootClass::Is_Cell_Harvestable @ 0x004DCE80`, in order: the cell is in
/// the playfield (`0x00578460(cell, 1)`); the mover can reach its zone
/// (`MapClass::Can_Reach_Zone @ 0x0056D100` from the mover's cell, with its
/// type's MovementZone and its ShouldBeOnBridge answer, no destination
/// bridge and no fringe shortcut); it is Tiberium land; and Can_Enter_Cell
/// answers MOVE_OK (`vt+0x1AC(cell, -1, -1, 0, 1)`). Answers the cell.
///
/// The source cell is the mover's `vt+0x4C` coordinate, its NavCom
/// destination when it holds one; every caller scans without a NavCom, so
/// it is the mover's own cell, and ShouldBeOnBridge (`0x005F6A70` through
/// `0x004DDC40`) answers the mover's OnBridge byte.
///
/// RESIDUAL: a campaign (GameMode 0) player-owned mover (`Techno+0x41A`)
/// skips a shrouded cell (`0x004DCEA0..0x004DCF20`); not represented, so a
/// campaign miner may head for ore under the shroud. Skirmish never reads it.
fn is_cell_harvestable(
    sim: &Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    id: u64,
    own: (u16, u16),
    candidate: (i32, i32),
) -> Option<(u16, u16)> {
    let terrain = sim.resolved_terrain.as_ref();
    if !crate::sim::cell_rect::cell_is_in_playfield_height_aware(
        candidate,
        sim.playfield_bounds,
        terrain,
    ) {
        return None;
    }
    let cell = (
        u16::try_from(candidate.0).ok()?,
        u16::try_from(candidate.1).ok()?,
    );
    let entity = sim.substrate.entities.get(id)?;
    if let Some(zones) = sim.zone_grid.as_ref() {
        let source = (i32::from(own.0), i32::from(own.1));
        let movement_zone = sim
            .object_type(entity.type_ref(), rules)
            .map(|object| object.movement_zone)
            .filter(|&zone| zone != MovementZone::Invalid);
        // The exact Can_Reach_Zone surface; Is_Cell_Harvestable passes the
        // same argument shape as the base-defence response.
        if !zones.can_reach_base_defense_response(
            movement_zone,
            source,
            candidate,
            entity.on_bridge,
            crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                source,
                sim.playfield_bounds,
                terrain,
            ),
            i32::from(sim.session.map_width),
            i32::from(sim.session.map_height),
        ) {
            return None;
        }
    }
    if !cell_is_tiberium_land(sim, overlay_registry, cell) {
        return None;
    }
    (crate::sim::movement::unit_can_enter_cell_standing(sim, id, cell, rules) == 0).then_some(cell)
}

#[cfg(test)]
mod tests {
    use super::scan_cells;

    #[test]
    fn scan_radius_divides_leptons_toward_zero() {
        assert_eq!(scan_cells(0x600), 6);
        assert_eq!(scan_cells(1664), 6);
        assert_eq!(scan_cells(-1664), -6);
        assert_eq!(scan_cells(255), 0);
    }
}
