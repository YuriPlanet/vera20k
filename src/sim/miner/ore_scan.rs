//! The Foot tiberium search Mission_Harvest runs for a harvester:
//! `FootClass::Search_For_Tiberium_And_Move @ 0x004DCFE0`,
//! `FootClass::Scan_For_Tiberium @ 0x004DD0A0` (Unit and Infantry vtable
//! `+0x338`) and `FootClass::Is_Cell_Harvestable @ 0x004DCE80`; and the
//! Building's `TechnoClass::Scan_For_Tiberium @ 0x0070F8F0`, which a Slave
//! Miner refinery runs for its relocation.
//!
//! Evidence: `tools/spatial_oracle/harvest_field.json` `scan` and `search`
//! rows, replayed by `world/harvest_field_oracle_tests.rs`; the Building
//! body runs natively in `tools/spatial_oracle/slave_manager.json`'s state-5
//! rows (`world/slave_manager_oracle_tests.rs`).
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
pub(crate) fn scan_cells(leptons: i32) -> i32 {
    leptons.wrapping_add((leptons >> 31) & 0xFF) >> 8
}

/// Whether `cell` holds LandType 5 (`CellClass+0xEC`), the Tiberium land an
/// ore or gem overlay gives its cell. The resolved terrain's land type is the
/// authority; a fixture with no resolved terrain reads the overlay's own
/// `Land=`, the value `CellClass::RecalcAttributes @ 0x0047D2B0` writes for a
/// tiberium overlay (`NoUseTileLandType` is set).
pub(crate) fn cell_is_tiberium_land(
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

/// `Scan_For_Tiberium(range)`, the `vt+0x338` virtual: FootClass's body for
/// a Unit or Infantry ([`foot_scan_for_tiberium`]), TechnoClass's for a
/// Building ([`techno_scan_for_tiberium`]). Its third argument (0 for
/// `SlaveMinerShortScan`, 1 for `SlaveMinerLongScan` at `0x006B006B` and
/// `0x006B00BB`) is read by neither body. `None` is the `(0,0)` no-cell
/// answer.
pub(crate) fn scan_for_tiberium(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    id: u64,
    range: i32,
) -> Option<(u16, u16)> {
    let entity = sim.substrate.entities.get(id)?;
    if entity.category == crate::map::entities::EntityCategory::Structure {
        return techno_scan_for_tiberium(sim, rules, overlay_registry, id, range);
    }
    foot_scan_for_tiberium(sim, rules, overlay_registry, id, range)
}

/// `FootClass::Scan_For_Tiberium @ 0x004DD0A0`: the mover's own cell when
/// it is Tiberium land; else the rings ([`best_in_rings`]) over the cells
/// Is_Cell_Harvestable admits. `None` is the `(0,0)` no-cell answer
/// (`0x008B3D88`).
fn foot_scan_for_tiberium(
    sim: &mut Simulation,
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
    let reach = harvest_reach(sim, rules, id)?;
    best_in_rings(sim, own, range, |sim, candidate| {
        let cell = is_cell_harvestable(sim, rules, overlay_registry, id, reach, candidate)?;
        Some((cell, tiberium_value(sim, rules, overlay_registry, cell)))
    })
}

/// `TechnoClass::Scan_For_Tiberium @ 0x0070F8F0`, a Building's: the same
/// rings around its GetCoords cell (the foundation centre,
/// `BuildingClass::GetCoords @ 0x00447AC0`), where a candidate needs only
/// LandType 5 (`0x00487DF0`, `0x0070F9C3`): no playfield, zone or occupancy
/// test. `None` is the `(0,0)` no-cell answer (`0x00B0EA50`).
fn techno_scan_for_tiberium(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    id: u64,
    range: i32,
) -> Option<(u16, u16)> {
    let (x, y, _, _) = crate::sim::combat::resolve_target_coords(
        &crate::sim::combat::TargetKind::Entity(id),
        &sim.substrate.entities,
        Some(rules),
        &sim.interner,
    )?;
    let own = (x, y);
    if cell_is_tiberium_land(sim, overlay_registry, own) {
        return Some(own);
    }
    best_in_rings(sim, own, range, |sim, (cx, cy)| {
        let cell = (u16::try_from(cx).ok()?, u16::try_from(cy).ok()?);
        cell_is_tiberium_land(sim, overlay_registry, cell)
            .then(|| (cell, tiberium_value(sim, rules, overlay_registry, cell)))
    })
}

/// The ring walk both Scan_For_Tiberium bodies share: rings `r = 1..range`
/// around `own` (the bound itself is not scanned), each walking `i = -r..=r`
/// over `(x+i, y-r)`, `(x+i, y+r)`, `(x-r, y+i)`, `(x+r, y+i)` (corners
/// twice). A candidate the body admits (`value_at` answers its cell and
/// Tiberium value, `0x00485020`) whose value beats the best so far
/// (strictly, so ties keep the earlier cell) becomes the best, and the first
/// ring with a hit ends the scan.
fn best_in_rings(
    sim: &mut Simulation,
    own: (u16, u16),
    range: i32,
    mut value_at: impl FnMut(&mut Simulation, (i32, i32)) -> Option<((u16, u16), i32)>,
) -> Option<(u16, u16)> {
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
                let Some((cell, value)) = value_at(sim, candidate) else {
                    continue;
                };
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

/// The `MapClass::Can_Reach_Zone @ 0x0056D100` request Is_Cell_Harvestable
/// makes for every candidate of one scan (`0x004DCF26..0x004DCF92`): the
/// cell of the mover's `vt+0x4C` coordinate (`FootClass::GetDestination @
/// 0x004DBDF0`: a tube's exit, else the locomotor's head-to, else the mover's
/// own coordinate), its type's MovementZone (`TechnoType+0x5B4`) and its
/// ShouldBeOnBridge answer (`vt+0xBC`, `0x004DDC40`: the OnBridge byte).
/// Is_Cell_Harvestable passes no destination bridge and no fringe shortcut,
/// the same shape as the base-defence response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HarvestReach {
    pub(crate) source: (i32, i32),
    pub(crate) movement_zone: Option<MovementZone>,
    pub(crate) source_on_bridge: bool,
}

pub(crate) fn harvest_reach(sim: &Simulation, rules: &RuleSet, id: u64) -> Option<HarvestReach> {
    let entity = sim.substrate.entities.get(id)?;
    // `CDQ; AND EDX,0xFF; ADD; SAR 8` (`0x004DCF3E..0x004DCF5D`).
    let cell = |leptons: i32| leptons.wrapping_add((leptons >> 31) & 0xFF) >> 8;
    let source = match sim.foot_navigation_coordinate(id) {
        Ok(coord) => (cell(coord.x), cell(coord.y)),
        // VERA builds a Drive's runtime on its first move: until then it
        // holds no head-to, and Head_To_Coord answers the mover's own
        // coordinate (`0x004AFD0B`).
        Err(_) if entity.low_bridge_tube_state.is_none() => {
            (i32::from(entity.position.rx), i32::from(entity.position.ry))
        }
        Err(_) => return None,
    };
    Some(HarvestReach {
        source,
        movement_zone: sim
            .object_type(entity.type_ref(), rules)
            .map(|object| object.movement_zone)
            .filter(|&zone| zone != MovementZone::Invalid),
        source_on_bridge: entity.on_bridge,
    })
}

/// `FootClass::Is_Cell_Harvestable @ 0x004DCE80`: the cell is in the
/// playfield (`0x00578460(cell, 1)`); the mover can reach its zone
/// ([`HarvestReach`]); it is Tiberium land; and the class's own
/// Can_Enter_Cell answers MOVE_OK (`vt+0x1AC(cell, -1, -1, 0, 1)`,
/// [`Simulation::foot_can_enter`]). Answers the cell.
///
/// Every test is a pure query, so the cheap LandType test runs before the
/// zone lookup; the answer is the native conjunction's.
///
/// RESIDUAL: a campaign (GameMode 0) player-owned mover (`Techno+0x41A`)
/// skips a shrouded cell (`0x004DCEA0..0x004DCF20`); not represented, so a
/// campaign miner may head for ore under the shroud. Skirmish never reads it.
fn is_cell_harvestable(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    id: u64,
    reach: HarvestReach,
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
    if !cell_is_tiberium_land(sim, overlay_registry, cell) {
        return None;
    }
    if let Some(zones) = sim.zone_grid.as_ref()
        && !zones.can_reach_base_defense_response(
            reach.movement_zone,
            reach.source,
            candidate,
            reach.source_on_bridge,
            crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                reach.source,
                sim.playfield_bounds,
                terrain,
            ),
            i32::from(sim.session.map_width),
            i32::from(sim.session.map_height),
        )
    {
        return None;
    }
    let native_cell = sim
        .resolved_terrain
        .as_ref()?
        .native_cell_identity((cell.0 as i16, cell.1 as i16));
    let code = sim
        .foot_can_enter(
            id,
            native_cell,
            crate::sim::movement::infantry_entry::InfantryEntryArgs::REPAIR,
            rules,
            overlay_registry,
        )
        .ok()?;
    (code == 0).then_some(cell)
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
