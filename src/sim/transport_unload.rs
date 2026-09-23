//! Transport passenger unload — the Unload mission handlers for vehicle and
//! aircraft transports (`Passengers > 0`).
//!
//! Vehicle: the `Type+0x5E0 > 0` branch of `UnitClass::Mission_Unload @
//! 0x0073D630` (vtable `+0x23C` at `0x007F5EAC`), states on `unit+0xBC`
//! through the jump table at `0x0073E5C0`, body `0x0073D70E`..`0x0073DCCE`.
//! Aircraft: the Aircraft Unload slot `0x004151E0` (vtable `+0x23C` at
//! `0x007E24E0`; the Ghidra label `AircraftClass__Mission_Hunt` is wrong).
//!
//! Every dispatch that does not `return 10` / `return 1` early exits through
//! the common epilogue `ftol([Unload] Rate × 900) + RandomRanged(0, 2)`
//! (`0x0073E289`..`0x0073E2B5`; aircraft tail at the end of `0x004151E0`), so
//! one passenger leaves per 14..16 frames on stock `[Unload] Rate=.016`.
//!
//! Depends on `sim/`, `rules/`, `map/` only — never render/ui/sidebar/audio/net.

use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_rect::{
    IsClearToMoveResult, LiveCellPassabilityQuery, evaluate_live_cell_passability,
};
use crate::sim::find_nearby_cell::{
    NearbyAnchorGate, NearbyFootprint, NearbyQuery, NearbySearchOptions, PassabilityArgs,
    RADIUS_HARD_CAP, find_nearby_passable_cell_with_options,
};
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::authority::EntityReadyInputProvider;
use crate::sim::mission::{DockTeardown, MissionId, MissionType};
use crate::sim::movement::FacingClass;
use crate::sim::movement::bump_crush;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::movement::ready_producer::is_moving_now_for;
use crate::sim::passenger::{
    DepartureFailure, DepartureRoute, depart_cargo_head, reveal_unloaded_passenger,
};
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::passability::LandType;
use crate::sim::world::{SimSoundEvent, Simulation};
use crate::util::fixed_math::SIM_ZERO;
use crate::util::lepton::CELL_CENTER_LEPTON;

/// `g_DirectionOffsets @ 0x0089F688`: eight `(dx, dy)` cell deltas indexed by
/// octant (facing >> 13). Index 6 is `g_dwDirectionOffset_W @ 0x0089F6A0` =
/// `(-1, 0)`, the same table the refinery-unload branch of the same handler
/// uses to find the refinery west of the dock pad; facing East (`0x4000`,
/// octant 2) is `+x`.
const OCTANT_OFFSETS: [(i32, i32); 8] = [
    (0, -1),  // 0 N
    (1, -1),  // 1 NE
    (1, 0),   // 2 E
    (1, 1),   // 3 SE
    (0, 1),   // 4 S
    (-1, 1),  // 5 SW
    (-1, 0),  // 6 W
    (-1, -1), // 7 NW
];

/// `DAT_008458D0`: `[4, 5, 6, 7, 0, 1, 2, 3]` — the octant the transport
/// turns to so that its REAR faces the chosen exit cell.
const fn opposite_octant(octant: usize) -> usize {
    (octant + 4) & 7
}

/// Handler states on `unit+0xBC` (`MissionCom::handler_state`).
const STATE_PICK_EXIT: u32 = 0;
const STATE_TURNING: u32 = 1;
const STATE_EJECT: u32 = 3;
const STATE_DONE: u32 = 4;

/// Aircraft handler states on `aircraft+0xBC`. Native state 1 (written only
/// at `0x0041541F`, inside the team arm) is EXCLUDED: VERA has no teams.
const AIR_STATE_CHECK_LANDED: u32 = 0;
const AIR_STATE_WAIT_STOP: u32 = 2;
const AIR_STATE_EJECT: u32 = 3;
const AIR_STATE_RESET: u32 = 4;

/// Early return `0xA` — "still moving / driving to land, ask again in 10".
const WAIT_MOVING_FRAMES: i32 = 10;

/// `ftol([Unload] Rate × 900) + RandomRanged(0, 2)` on the Scenario stream
/// (`Random__RandomRanged` at `0x0073E2B0` on `*(0x00A8B230)+0x218`). The
/// base is computed FIRST and consumes no RNG; the draw follows.
fn unload_epilogue(sim: &mut Simulation, rules: &RuleSet) -> i32 {
    let base = rules
        .mission_control
        .rate_frames(MissionType::Unload)
        .min(i32::MAX as u32) as i32;
    let jitter = sim.scenario_rng.next_range_u32_inclusive(0, 2) as i32;
    base.saturating_add(jitter)
}

/// Whether this entity's type takes the transport branch of the Unit Unload
/// handler (`Type+0x5E0 Passengers > 0`, gate at `0x0073D6EC`).
pub(crate) fn is_vehicle_transport_type(
    sim: &Simulation,
    entity: &GameEntity,
    rules: &RuleSet,
) -> bool {
    sim.object_type(entity.type_ref(), rules)
        .is_some_and(|obj| obj.passengers > 0)
}

fn cargo_count(entity: &GameEntity) -> u32 {
    entity
        .passenger_role
        .cargo()
        .map_or(0, |cargo| cargo.count())
}

fn cell_from(base: (u16, u16), delta: (i32, i32)) -> Option<(u16, u16)> {
    let x = i32::from(base.0) + delta.0;
    let y = i32::from(base.1) + delta.1;
    (x >= 0 && y >= 0 && x <= i32::from(u16::MAX) && y <= i32::from(u16::MAX))
        .then_some((x as u16, y as u16))
}

fn cell_in_map(sim: &Simulation, path_grid: Option<&PathGrid>, cell: (u16, u16)) -> bool {
    if let Some(terrain) = sim.resolved_terrain.as_ref() {
        return terrain.cell(cell.0, cell.1).is_some();
    }
    if let Some(grid) = path_grid {
        return cell.0 < grid.width() && cell.1 < grid.height();
    }
    true
}

/// Terrain level of a cell for the reveal Z, or the transport's own Z.
fn cell_level_or(sim: &Simulation, cell: (u16, u16), fallback: u8) -> u8 {
    sim.resolved_terrain
        .as_ref()
        .and_then(|terrain| terrain.cell(cell.0, cell.1))
        .map_or(fallback, |c| c.level)
}

/// The FNPC `allow_bridge_cells == 0` reject and the placement skip at
/// `0x0073D9EB` (`CellClass+0x140 & 0x100`, the structural bridge flag).
fn cell_has_structural_bridge(
    sim: &Simulation,
    path_grid: Option<&PathGrid>,
    cell: (u16, u16),
) -> bool {
    path_grid
        .and_then(|grid| grid.cell(cell.0, cell.1))
        .is_some_and(|c| c.has_structural_bridge())
        || sim
            .resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.cell(cell.0, cell.1))
            .is_some_and(|c| c.bridge_facts.has_structural_bridge())
}

/// Land passability of a cell for one speed type, from the resolved terrain
/// speed table (a zero cost is impassable), falling back to the path grid.
fn land_passable_for(
    sim: &Simulation,
    path_grid: Option<&PathGrid>,
    cell: (u16, u16),
    speed_type: SpeedType,
) -> bool {
    match sim
        .resolved_terrain
        .as_ref()
        .and_then(|terrain| terrain.cell(cell.0, cell.1))
    {
        Some(terrain_cell) => terrain_cell
            .speed_costs
            .cost_for_speed_type(speed_type)
            .is_none_or(|cost| cost > 0),
        None => path_grid.is_none_or(|grid| grid.is_walkable(cell.0, cell.1)),
    }
}

/// One octant candidate of the exit-cell scorer `FUN_00740B60` (UnitClass
/// vtable `+0x304`, called with a NULL passenger at `0x0073D7F4`): the cell's
/// LandType row must have a non-zero Foot column, the cell must carry no
/// vehicle/building occupation and must not be full of infantry
/// (`CellClass+0x124 & 0xE0 == 0`, `& 0x1F != 0x1F`), and the nearest object
/// in it, when any, must be an ally of the transport's owner
/// (`HouseClass::Is_Ally_ByObject`, `0x00740CE7`..`0x00740CF3`).
///
/// VERA-internal: the Rust cell holds three infantry sub-cells, so "full" is
/// the sub-cell allocator's refusal; every ground occupant is alliance-tested
/// rather than only the nearest one.
fn octant_cell_open(
    sim: &Simulation,
    path_grid: Option<&PathGrid>,
    cell: (u16, u16),
    transport_owner: &str,
) -> bool {
    if !cell_in_map(sim, path_grid, cell) {
        return false;
    }
    if !land_passable_for(sim, path_grid, cell, SpeedType::Foot) {
        return false;
    }
    let occupancy = sim.substrate.occupancy.get(cell.0, cell.1);
    if !bump_crush::cell_passable_for_infantry(occupancy, MovementLayer::Ground) {
        return false;
    }
    if let Some(occ) = occupancy {
        for occupant in occ.iter_layer(MovementLayer::Ground) {
            let Some(other) = sim.substrate.entities.get(occupant.entity_id) else {
                continue;
            };
            let other_owner = sim.interner.resolve(other.owner());
            if !crate::map::houses::is_allied_with(
                &sim.house_alliances,
                transport_owner,
                other_owner,
            ) {
                return false;
            }
        }
    }
    true
}

/// `FUN_00740B60` with a NULL passenger: score the eight neighbour cells and
/// return `(exit cell, octant the transport must FACE)`.
///
/// Per octant (`0x00740BB0`..`0x00740D5B`): `0x80` when the cell is open,
/// `-0x80` otherwise; minus the absolute difference between the octant's
/// 8-bit facing (`(char)(octant << 5)`) and the 8-bit facing of the
/// transport's REAR (`(char)(((facing16 + 0x7FFF) >> 7 + 1) >> 1)`), both
/// taken as SIGNED bytes before the subtraction (so the south octant reads
/// `-128`, not `128`); octant 4 (S) is penalised by a further `-100`
/// (`0x00740D3B`). The first strictly-greatest score wins (`local_38 < score`,
/// initial `-1`). Only a positive best writes a cell (`0x00740D7E`); the
/// returned octant is `DAT_008458D0[best]` = the opposite octant, so the
/// transport ends up with its back to the exit.
///
/// The `UnitType+0xC94` (`IsTrain=`) override that returns the current facing
/// octant instead (`0x00740DC5`) is not represented: no stock YR unit sets
/// the key and it is not parsed.
fn pick_exit_octant(
    sim: &Simulation,
    path_grid: Option<&PathGrid>,
    entity: &GameEntity,
) -> Option<((u16, u16), usize)> {
    let owner = sim.interner.resolve(entity.owner());
    let base = (entity.position.rx, entity.position.ry);
    let facing16 = u16::from(entity.facing) << 8;
    let rear16 = facing16.wrapping_add(0x7FFF);
    let rear8 = i32::from((((u32::from(rear16) >> 7) + 1) >> 1) as u8 as i8);

    let mut best_score: i32 = -1;
    let mut best_octant: usize = 0;
    for octant in 0..8usize {
        let open = cell_from(base, OCTANT_OFFSETS[octant])
            .is_some_and(|cell| octant_cell_open(sim, path_grid, cell, owner));
        let mut score: i32 = if open { 0x80 } else { -0x80 };
        let dir8 = i32::from(((octant as u32) << 5) as u8 as i8);
        score -= (dir8 - rear8).abs();
        if octant == 4 {
            score -= 100;
        }
        if best_score == -1 || best_score < score {
            best_score = score;
            best_octant = octant;
        }
    }
    if best_score <= 0 {
        return None;
    }
    let cell = cell_from(base, OCTANT_OFFSETS[best_octant])?;
    Some((cell, opposite_octant(best_octant)))
}

/// `Do_Turn(octant << 13)` on the transport's hull (`0x0073D86C`, locomotor
/// slot `+0x4C`): arm the body `FacingClass` toward the 8-bit facing
/// `octant * 32` at the unit's `ROT=`.
fn start_hull_turn(entity: &mut GameEntity, octant: usize, now: u32) {
    let target8 = ((octant as u32) << 5) as u8;
    let rot = entity.locomotor.as_ref().map_or(0, |loco| loco.rot);
    entity.facing_target = Some(target8);
    let mut body = FacingClass::new(u16::from(entity.facing) << 8, rot);
    body.set(u16::from(target8) << 8, now);
    entity.body_facing = Some(body);
}

/// State 1's `+0x6AF == 0` read (`0x0073D892`): the hull turn has finished.
fn hull_turn_finished(entity: &GameEntity, now: u32) -> bool {
    entity
        .body_facing
        .as_ref()
        .is_none_or(|body| !body.is_rotating(now))
}

/// Per-tick `PrimaryFacing.Current()` refresh for a transport turning in
/// place for its unload. The movement tick only rotates objects that hold a
/// movement target, so the idle hull turn armed by [`start_hull_turn`] is
/// advanced here from the same frame-anchored `FacingClass` gamemd reads.
/// VERA-internal representation of a native pure-function read.
pub(crate) fn refresh_idle_hull_turn(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    if !sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|e| is_vehicle_transport_type(sim, e, rules))
    {
        return;
    }
    let now = sim.session.binary_frame;
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return;
    };
    if entity.mission.current() != MissionId::from_known(MissionType::Unload)
        || entity.mission.handler_state() != STATE_TURNING
        || entity.movement_target.is_some()
    {
        return;
    }
    let Some(body) = entity.body_facing.as_ref() else {
        return;
    };
    entity.facing = (body.current(now) >> 8) as u8;
}

fn finish_hull_turn(entity: &mut GameEntity) {
    if let Some(target) = entity.facing_target.take() {
        entity.facing = target;
    }
    entity.body_facing = None;
}

/// `ObjectClass::IsCellOccupied` (vtable `+0x1AC`) for the ejected passenger at
/// one cell, as called at `0x0073D994` / `0x0073D9C2` with
/// `(cell, octant, level, NULL, alt=1)`. A zero return is "may enter".
fn passenger_can_enter(
    sim: &Simulation,
    rules: &RuleSet,
    path_grid: Option<&PathGrid>,
    passenger: &GameEntity,
    cell: (u16, u16),
) -> bool {
    if !cell_in_map(sim, path_grid, cell) {
        return false;
    }
    let obj = sim.object_type(passenger.type_ref(), rules);
    let speed_type = passenger
        .locomotor
        .as_ref()
        .map(|loco| loco.speed_type)
        .or_else(|| obj.map(|o| o.speed_type))
        .unwrap_or(SpeedType::Foot);
    let movement_zone = obj.map_or(MovementZone::Normal, |o| o.movement_zone);
    let land_passable = land_passable_for(sim, path_grid, cell, speed_type);
    if path_grid.is_none() && sim.resolved_terrain.is_none() {
        // Headless fixtures have no Cell substrate: only the occupancy verdict
        // is available.
        let occupancy = sim.substrate.occupancy.get(cell.0, cell.1);
        return if passenger.category == EntityCategory::Infantry {
            bump_crush::cell_passable_for_infantry(occupancy, MovementLayer::Ground)
        } else {
            occupancy.is_none_or(|occ| occ.is_empty_on(MovementLayer::Ground))
        };
    }
    matches!(
        evaluate_live_cell_passability(LiveCellPassabilityQuery {
            target: cell,
            speed_type,
            movement_zone,
            requested_zone: None,
            actual_zone: 0,
            requested_layer: None,
            ignore_infantry: false,
            ignore_vehicles: false,
            land_passable,
            path_grid,
            resolved_terrain: sim.resolved_terrain.as_ref(),
            raw_occupation: Some(&sim.substrate.raw_cell_occupation),
        }),
        IsClearToMoveResult::Clear { .. } | IsClearToMoveResult::ClearWinged
    )
}

/// `FootClass::Find_Nearby_Passable_Cell` seeded at `seed` for `mover`'s own
/// speed type — the state-0 water→land pre-move (`0x0073D7B0`) and the
/// vehicle-passenger placement search (`0x0073DADD`, seeded at the exit cell
/// with the passenger's `Type+0x67C` SpeedType). Bridge cells are refused
/// and the terrain-level gate is on; the remaining flag arguments are not
/// individually decoded (bounded reuse of the free-unit query shape).
fn find_nearby_passable_for(
    sim: &Simulation,
    rules: &RuleSet,
    path_grid: Option<&PathGrid>,
    mover: &GameEntity,
    seed: (u16, u16),
    speed_type_override: Option<SpeedType>,
) -> Option<(u16, u16)> {
    let obj = sim.object_type(mover.type_ref(), rules)?;
    let speed_type = speed_type_override.unwrap_or_else(|| {
        mover
            .locomotor
            .as_ref()
            .map_or(obj.speed_type, |loco| loco.speed_type)
    });
    let query = NearbyQuery {
        native_cells: None,
        raw_occupation: None,
        passability: PassabilityArgs {
            speed_type,
            required_zone_id: None,
            movement_zone: obj.movement_zone,
            bridge_aware_zone: false,
        },
        footprint: NearbyFootprint::SINGLE,
        anchor_gate: NearbyAnchorGate::UnverifiedCompatibilityBypass,
        allow_bridge_cells: false,
        check_height: true,
        check_occupancy: false,
        radius_cap: RADIUS_HARD_CAP,
        target_cell: None,
        path_grid,
        resolved_terrain: sim.resolved_terrain.as_ref(),
        overlay_grid: sim.overlay_grid.as_ref(),
        occupancy: Some(&sim.substrate.occupancy),
        entities: Some(&sim.substrate.entities),
        zone_grid: sim.zone_grid.as_ref(),
        playfield_bounds: sim.playfield_bounds,
    };
    find_nearby_passable_cell_with_options(
        (i32::from(seed.0), i32::from(seed.1)),
        &query,
        NearbySearchOptions::default(),
        sim.session.binary_frame,
    )
}

/// `Set_Destination(cell, 1)` for an object the handler wants driven — the
/// production representation is the same pathed move `Command::Move` issues.
/// Without a path grid (headless fixtures) nothing moves.
fn issue_pathed_move(
    sim: &mut Simulation,
    rules: &RuleSet,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    id: u64,
    dest: (u16, u16),
) {
    let Some(grid) = path_grid else {
        return;
    };
    let Some(info) = sim.resolve_move_info(id, Some(rules)) else {
        return;
    };
    let owner = sim
        .substrate
        .entities
        .get(id)
        .map(|entity| sim.interner.resolve(entity.owner()).to_string())
        .unwrap_or_default();
    let (entity_blocks, entity_block_map) = bump_crush::build_entity_block_set(
        &sim.substrate.entities,
        &owner,
        &sim.house_alliances,
        &sim.interner,
        Some(rules),
    );
    let cost_grid = sim.terrain_costs.get(&info.speed_type);
    let blocker_neighbor_counts = bump_crush::build_blocker_neighbor_counts_with_overlays(
        &sim.substrate.entities,
        grid.width(),
        grid.height(),
        sim.resolved_terrain.as_ref(),
        sim.overlay_grid.as_ref(),
        overlay_registry,
        &sim.interner,
        Some(rules),
    );
    let _ = crate::sim::movement::issue_move_command_with_layered(
        &mut sim.substrate.entities,
        grid,
        id,
        dest,
        info.speed,
        false,
        cost_grid,
        Some(&entity_blocks),
        sim.resolved_terrain.as_ref(),
        sim.zone_grid.as_ref(),
        Some(&entity_block_map),
        Some(&blocker_neighbor_counts),
        sim.playfield_bounds,
        Some(&mut sim.substrate.cell_occupation),
        crate::sim::movement::DestinationTiming::from_rules(sim.session.binary_frame, rules.into()),
    );
}

fn queue_guard(sim: &mut Simulation, id: u64) {
    let now = sim.session.binary_frame;
    let _ = sim.mission_queue_exact(
        id,
        MissionId::from_known(MissionType::Guard),
        0,
        now,
        &EntityReadyInputProvider,
    );
}

/// Outcome of one state-3 ejection attempt.
enum EjectOutcome {
    /// Placed at the exit cell; the passenger was given `dest`.
    Placed,
    /// No octant accepted the passenger; it was put back at the cargo head.
    Failed,
}

/// State 3 (`0x0073D8B7`..`0x0073DCA6`): pop the head passenger, scan the
/// eight octants for an exit cell and place it.
///
/// Scan start `((facing16 + 0x7FFF) >> 12 + 1) >> 1 & 7` (`0x0073D8F4`..
/// `0x0073D922`) — the octant BEHIND the transport, which state 0 turned to
/// face away from the exit cell. Two passes: with the retry byte set
/// (`[ESP+0x12] = 1`, `0x0073D8EF`) a candidate needs BOTH the adjacent cell
/// and the cell beyond it enterable; the first time the scan hits `i == 7`
/// still rejecting, the byte clears and the scan restarts at `i = 1`
/// (`0x0073DA15`..`0x0073DA27`), accepting an adjacent cell alone. A
/// candidate carrying the structural bridge flag is skipped without the
/// wrap (`0x0073D9EB`..`0x0073DA02`). The placement coordinate is the exit
/// cell's centre; infantry route through `PlaceInfantryInCell`
/// (`0x0073DA7C`), vehicles through `Find_Nearby_Passable_Cell` seeded at the
/// exit cell (`0x0073DADD`), whose result overwrites the exit-cell slot
/// `[ESP+0x14]` (`0x0073DAE8`). `Unlimbo(coord, octant * 32)` follows
/// (`0x0073DB6A`); on success the passenger's transporter link `+0x11C` is
/// cleared, `Queue_Mission(Move)` (`0x0073DBDB`) and `Set_Destination` to the
/// cell BEYOND (`[ESP+0x30]`) on the strict pass or to `[ESP+0x14]` on the
/// relaxed pass (`0x0073DBE1`..`0x0073DC06`) — the exit cell for infantry,
/// the FNPC placement cell for a vehicle passenger — and `LeaveTransportSound`
/// plays at the transport (`0x0073DC28`..`0x0073DC67`). Any failure
/// re-inserts the passenger with `AddPassenger` (`0x0073DC78`) and re-applies
/// the gunner weapon (`0x0073DC96`).
///
/// Infantry `Unlimbo` (`InfantryClass::Unlimbo @ 0x0051DFF0`) runs
/// `PlaceInfantryInCell` a second time from the already-adjusted sub-cell
/// coordinate, but draws no RNG there: the handler increments
/// `g_MapEditorMode` (`0x00A8E7AC`) at `0x0073DA48` before the `+0xD8`
/// `Unlimbo` call (`0x0073DB6A`) and decrements it at `0x0073DB79` after,
/// so `InfantryClass::Unlimbo` passes `priority = 1` and
/// `CellClass::PlaceInfantryInCell @ 0x00481180` takes the no-occupancy-test
/// branch (`LAB_00481437`: offset-table lookup, no `RandomRanged(0, 3)`).
/// The single draw here is therefore the whole native budget. The infantry
/// placement writes only the coordinate (`0x0073DA83`), leaving the exit
/// cell in `[ESP+0x14]` for the relaxed-pass destination.
fn eject_head_passenger(
    sim: &mut Simulation,
    rules: &RuleSet,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    transport_id: u64,
) -> EjectOutcome {
    match depart_cargo_head(
        sim,
        rules,
        transport_id,
        DepartureRoute::Vehicle,
        |sim, pax_id| {
            let (base, facing, transport_z, leave_sound) = {
                let transport = sim
                    .substrate
                    .entities
                    .get(transport_id)
                    .expect("transport resolved before the head pop");
                let obj = sim.object_type(transport.type_ref(), rules);
                (
                    (transport.position.rx, transport.position.ry),
                    transport.facing,
                    transport.position.z,
                    obj.and_then(|o| o.leave_transport_sound.clone()),
                )
            };
            let Some(passenger_snapshot) = sim.substrate.entities.get(pax_id) else {
                // A cargo id that no longer resolves cannot be placed; keep the
                // native failure shape (re-insert) so the count stays coherent.
                return Err(DepartureFailure::MissingPassenger);
            };
            let passenger_is_infantry = passenger_snapshot.category == EntityCategory::Infantry;

            let facing16 = u16::from(facing) << 8;
            let start =
                ((((u32::from(facing16.wrapping_add(0x7FFF))) >> 12) + 1) >> 1) as usize & 7;
            let mut strict_pass = true;
            let mut i: usize = 0;
            // `(exit cell, beyond cell on the strict pass, octant)`.
            let mut placement: Option<((u16, u16), Option<(u16, u16)>, usize)> = None;
            while i < 8 {
                let octant = (start + i) & 7;
                let exit_cell = cell_from(base, OCTANT_OFFSETS[octant]);
                let beyond_cell =
                    exit_cell.and_then(|cell| cell_from(cell, OCTANT_OFFSETS[octant]));
                let (adjacent_ok, beyond_ok) = match (exit_cell, beyond_cell) {
                    (Some(exit), Some(beyond)) => {
                        let passenger = sim
                            .substrate
                            .entities
                            .get(pax_id)
                            .expect("passenger resolved above");
                        (
                            passenger_can_enter(sim, rules, path_grid, passenger, exit),
                            passenger_can_enter(sim, rules, path_grid, passenger, beyond),
                        )
                    }
                    _ => (false, false),
                };
                let accept = adjacent_ok && (beyond_ok || !strict_pass);
                if !accept {
                    if strict_pass && i == 7 {
                        strict_pass = false;
                        i = 1;
                    } else {
                        i += 1;
                    }
                    continue;
                }
                let (exit, beyond) = (
                    exit_cell.expect("accepted cell exists"),
                    beyond_cell.expect("accepted cell exists"),
                );
                if cell_has_structural_bridge(sim, path_grid, exit) {
                    i += 1;
                    continue;
                }
                placement = Some((exit, strict_pass.then_some(beyond), octant));
                break;
            }

            let Some((exit, strict_beyond, octant)) = placement else {
                return Err(DepartureFailure::Placement);
            };

            // Placement coordinate: the exit cell centre (`x * 256 + 0x80`).
            let (place_cell, sub_cell) = if passenger_is_infantry {
                // `PlaceInfantryInCell` from the cell centre: quadrant 0, so the
                // centre-row `RandomRanged(0, 3)` draw is made on the Scenario stream.
                let spot = bump_crush::place_infantry_in_cell(
                    &sim.substrate.raw_cell_occupation,
                    exit.0,
                    exit.1,
                    MovementLayer::Ground,
                    CELL_CENTER_LEPTON,
                    CELL_CENTER_LEPTON,
                    &mut sim.scenario_rng,
                );
                (exit, spot)
            } else {
                let passenger = sim
                    .substrate
                    .entities
                    .get(pax_id)
                    .expect("passenger resolved above");
                let cell = find_nearby_passable_for(sim, rules, path_grid, passenger, exit, None)
                    .unwrap_or(exit);
                (cell, None)
            };
            if passenger_is_infantry && sub_cell.is_none() {
                return Err(DepartureFailure::Placement);
            }
            // Relaxed pass: `[ESP+0x14]` — the exit cell for infantry, the FNPC
            // placement cell for a vehicle (`0x0073DAE8` overwrote the slot).
            let dest = strict_beyond.unwrap_or(place_cell);

            if let Some(passenger) = sim.substrate.entities.get_mut(pax_id) {
                passenger.sub_cell = sub_cell;
                passenger.facing = ((octant as u32) << 5) as u8;
                if let Some(loco) = passenger.locomotor.as_mut() {
                    loco.layer = MovementLayer::Ground;
                }
            }
            let z = cell_level_or(sim, place_cell, transport_z);
            reveal_unloaded_passenger(sim, transport_id, pax_id, place_cell.0, place_cell.1, z)?;

            // OpenTopped: `TechnoClass::ClearInOpenTransport` (`0x007104A0`) drops the
            // passenger's in-transport firing membership; VERA's open-topped registry
            // is the logic-object registration the Reveal above re-establishes.
            if let Some(passenger) = sim.substrate.entities.get_mut(pax_id) {
                passenger.attack_target = None;
                passenger.passively_acquired_target = false;
                passenger.order_intent = None;
            }
            sim.queue_megamission_with_teardown(pax_id, MissionType::Move, DockTeardown::None);
            issue_pathed_move(sim, rules, path_grid, overlay_registry, pax_id, dest);

            if let Some(sound) = leave_sound {
                let sound_id = sim.interner.intern(&sound);
                sim.sound_events.push(SimSoundEvent::LeaveTransport {
                    sound_id,
                    rx: base.0,
                    ry: base.1,
                });
            }
            Ok(())
        },
    ) {
        Ok(()) => EjectOutcome::Placed,
        Err(_) => EjectOutcome::Failed,
    }
}

/// The `Passengers > 0` branch of `UnitClass::Mission_Unload @ 0x0073D630`.
///
/// Returns the handler's dispatch delay; the caller writes the epilogue.
pub(crate) fn unit_mission_unload(
    sim: &mut Simulation,
    rules: &RuleSet,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    id: u64,
) -> i32 {
    let now = sim.session.binary_frame;
    let Some(entity) = sim.substrate.entities.get(id) else {
        return unload_epilogue(sim, rules);
    };
    match entity.mission.handler_state() {
        STATE_PICK_EXIT => {
            // `ILocomotion::Is_Moving` (`0x0073D729`) → `return 10`.
            if is_moving_now_for(entity, now) || entity.movement_target.is_some() {
                return WAIT_MOVING_FRAMES;
            }
            // No NavCom and the current cell's LandType is Water (`+0xEC == 2`,
            // `0x0073D769`): drive to the nearest passable cell first and
            // `return 10` — a hover transport unloads on land.
            let on_water = sim
                .resolved_terrain
                .as_ref()
                .and_then(|terrain| terrain.cell(entity.position.rx, entity.position.ry))
                .is_some_and(|cell| cell.yr_cell_land_type == LandType::Water.as_index());
            if entity.navigation.nav_com.is_none() && on_water {
                // `PUSH 0x2` at `0x0073D790`: the search runs for speed-type
                // index 2 — Wheel in the native `SpeedType` order (Foot 0,
                // Track 1, Wheel 2, Hover 3, ...), a land-only type — not for
                // the transport's own (hover) type, which is what makes a
                // hover transport leave the water before it unloads.
                let seed = (entity.position.rx, entity.position.ry);
                if let Some(cell) = find_nearby_passable_for(
                    sim,
                    rules,
                    path_grid,
                    entity,
                    seed,
                    Some(SpeedType::Wheel),
                ) {
                    // `Set_Destination` only — the committed selector stays
                    // Unload and state 0 re-runs once the drive has ended.
                    issue_pathed_move(sim, rules, path_grid, overlay_registry, id, cell);
                }
                return WAIT_MOVING_FRAMES;
            }
            let cargo = cargo_count(entity);
            let pick = pick_exit_octant(sim, path_grid, entity);
            if let (true, Some((_exit_cell, face_octant))) = (cargo != 0, pick) {
                // `FUN_0070DC60` → `Type+0x808 TurretCount > 0` (`0x00717880`):
                // a turreted transport (the IFV) keeps its last passenger —
                // `+0x6E4 = (cargo == 1 ? 0 : 1)` (`0x0073D82C`..`0x0073D83C`).
                let turreted = sim
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|obj| obj.turret_count > 0);
                if let Some(entity) = sim.substrate.entities.get_mut(id) {
                    if turreted {
                        entity.transport_unload_keep_count = if cargo == 1 { 0 } else { 1 };
                    }
                    start_hull_turn(entity, face_octant, now);
                    entity.mission.set_handler_state(STATE_TURNING);
                }
                return 1;
            }
            // Nothing to unload or nowhere to unload: `Queue_Mission(Guard)`
            // (`0x0073D887`) and the epilogue.
            queue_guard(sim, id);
            unload_epilogue(sim, rules)
        }
        STATE_TURNING => {
            if hull_turn_finished(entity, now) {
                if let Some(entity) = sim.substrate.entities.get_mut(id) {
                    finish_hull_turn(entity);
                    entity.mission.set_handler_state(STATE_EJECT);
                }
                return 1;
            }
            unload_epilogue(sim, rules)
        }
        STATE_EJECT => {
            if cargo_count(entity) > entity.transport_unload_keep_count {
                let _ = eject_head_passenger(sim, rules, path_grid, overlay_registry, id);
            } else if let Some(entity) = sim.substrate.entities.get_mut(id) {
                entity.mission.set_handler_state(STATE_DONE);
            }
            unload_epilogue(sim, rules)
        }
        STATE_DONE => {
            // `Queue_Mission(Guard)` then `+0xB8 = 1` (`0x0073DCC1`..`0x0073DCC7`).
            queue_guard(sim, id);
            if let Some(entity) = sim.substrate.entities.get_mut(id) {
                entity.mission.set_movement_bypass_latch();
            }
            unload_epilogue(sim, rules)
        }
        _ => unload_epilogue(sim, rules),
    }
}

/// Whether the aircraft is on the ground: `GetHeight() == 0` (vtable `+0x1C8`,
/// `0x004151F0`) and the flight-altitude float `+0x2E8 == 0.0` (`0x00415200`).
fn aircraft_landed(entity: &GameEntity) -> bool {
    entity
        .locomotor
        .as_ref()
        .is_none_or(|loco| loco.altitude == SIM_ZERO)
}

/// The Aircraft Unload slot `0x004151E0` (Nighthawk and any other landed
/// `Passengers > 0` aircraft). Timer-gated inside; writes its own epilogue.
///
/// Team-less state graph (`+0x5A4 == NULL`, the only case VERA carries):
///
/// - State 0 (`0x004151FB`): `GetHeight() == 0` (`0x004151FF`) and
///   `+0x2E8 == 0.0` (`0x0041520D`) → the team test at `0x00415228` jumps a
///   team-less aircraft straight to state 3 (`0x00415250`); the
///   destination-equals-position compare (`0x0041522A`..`0x0041524E`) runs
///   ONLY for a team. Not landed → `0x00415290`: the airfield branch needs
///   `Type+0xC95` = `IsDropship=` (`TechnoTypeClass::ReadINI`
///   `0x00712350`..`0x00712373`, key string `0x0084447C`; `AirportBound=` is
///   a different field, `AircraftType+0xE0D`, `0x0041CC6E`), and no retail
///   rulesmd.ini type sets `IsDropship`, so a team-less `[SHAD]` (Nighthawk,
///   `Passengers=5`, `Landable=yes`) ALWAYS takes state 2 (`0x0041530C`).
///   Both arms fall into the epilogue at `0x0041525A` (one
///   `RandomRanged(0, 2)` draw).
/// - State 3 early-out: `+0x418 != 0` returns before the hold test
///   (`0x004154BB`..`0x004154C3`); its writer and meaning are UNCHECKED and
///   VERA does not represent it.
/// - State 1 (`0x0041542A`): written only at `0x0041541F` inside the team
///   arm — EXCLUDED, unreachable without teams.
/// - State 2 (`0x00415480`): locomotor `Is_Moving` (`+0x10`, `0x0041549D`)
///   false → state 3 (`0x004154A4`); `return 1` either way (`0x004154B1`),
///   NO epilogue draw while it polls.
/// - State 3 (`0x004154BB`..): cargo empty → `Enter_Idle_Mode` (vtable
///   `+0x484` = `0x004176F0`); a non-carryall pops the cargo head
///   (`RemoveFirstPassenger @ 0x00473430`), leaves its own cell list, ejects
///   through vtable `+0x100` = `0x00415B10` (see [`eject_from_aircraft`]),
///   re-enters, and — when that emptied the hold — goes idle in the same
///   dispatch; epilogue draw.
/// - State 4 (`0x004155C0`): → 0, `return 1`.
pub(crate) fn dispatch_aircraft_unload(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) {
    let now = sim.session.binary_frame;
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    if entity.dying
        || entity.category != EntityCategory::Aircraft
        || entity.mission.current() != MissionId::from_known(MissionType::Unload)
        || !entity.mission.dispatch_timer().due(now)
    {
        return;
    }
    let delay = match entity.mission.handler_state() {
        AIR_STATE_CHECK_LANDED => {
            // Team-less: landed → 3 (`0x00415250`), otherwise → 2
            // (`0x0041530C`). The destination compare is team-only.
            let next = if aircraft_landed(entity) {
                AIR_STATE_EJECT
            } else {
                AIR_STATE_WAIT_STOP
            };
            if let Some(entity) = sim.substrate.entities.get_mut(id) {
                entity.mission.set_handler_state(next);
            }
            unload_epilogue(sim, rules)
        }
        AIR_STATE_WAIT_STOP => {
            // `Is_Moving` false (`0x0041549D`): for the Jumpjet locomotor that
            // is only true once it has landed — the native locomotor lands on
            // its own when the linked object's mission is Unload. VERA's
            // jumpjet does not yet auto-land on Unload (recorded residual), so
            // the landed altitude gate is applied here as well; an airborne
            // Nighthawk keeps waiting rather than dropping its cargo mid-air.
            if entity.movement_target.is_none() && aircraft_landed(entity) {
                if let Some(entity) = sim.substrate.entities.get_mut(id) {
                    entity.mission.set_handler_state(AIR_STATE_EJECT);
                }
            }
            1
        }
        AIR_STATE_EJECT => {
            if cargo_count(entity) == 0 {
                aircraft_enter_idle_mode(sim, id);
            } else {
                let ejected = eject_from_aircraft(sim, rules, path_grid, overlay_registry, id);
                let hold_empty = sim
                    .substrate
                    .entities
                    .get(id)
                    .is_some_and(|entity| cargo_count(entity) == 0);
                // `!ejected`: VERA's bounded escape from a refused ejection
                // (native loses the passenger and keeps ejecting the rest;
                // see [`eject_from_aircraft`]). The hold keeps the passenger
                // and the mission leaves through the same empty-hold exit, so
                // this dispatch's epilogue draw is the last one.
                if hold_empty || !ejected {
                    aircraft_enter_idle_mode(sim, id);
                }
            }
            unload_epilogue(sim, rules)
        }
        AIR_STATE_RESET => {
            if let Some(entity) = sim.substrate.entities.get_mut(id) {
                entity.mission.set_handler_state(AIR_STATE_CHECK_LANDED);
            }
            1
        }
        _ => unload_epilogue(sim, rules),
    };
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.mission.write_dispatch_epilogue(now as i32, delay);
    }
}

/// `AircraftClass::Enter_Idle_Mode @ 0x004176F0` for a landed, human-owned,
/// team-less transport whose hold just emptied (the `+0x484(0, 1)` calls at
/// `0x004155B5` — hold already empty on state-3 entry — and `0x004155A2`
/// — the ejection just emptied it).
///
/// Native trace: the mission is not suspended (`+0x1FC` = `0x005B3A10`,
/// `+0xB0 == -1`); `+0x3D4` is clear — `AircraftClass::Unlimbo @ 0x00414310`
/// sets it only for a type that is not (`Selectable=` `ObjectType+0x230` and
/// `Landable=` `AircraftType+0xE0A`) or whose weapon 0 carries `Camera=`
/// (`WeaponType+0x147`), which is the off-map paradrop/spy-plane class, never
/// a landable transport — so the landed branch (`GetDisplayLayer == 2`,
/// `0x0041ADC0`) runs `Set_Destination(NULL, 1)` (`+0x480` = `0x0041AA80`),
/// `Assign_Target(NULL)` (`+0x3C8` = `0x006FCDB0`) and, for a human house,
/// picks Guard (5). The airfield hunt is gated on `Ammo (+0x2FC) == 0`, which
/// a `[SHAD]` without `Ammo=` never satisfies.
///
/// Radio residual: the last ejection left the aircraft in radio contact with
/// the passenger (`Transmit_Message(HELLO)` `0x00415C2E`, then `UNLOADED`
/// `0x00415C3F`, which `TechnoClass::Receive_Radio @ 0x006F4AB0` answers
/// with TETHER `0x18`), so `In_Radio_Contact` steers the first idle pass to
/// Enter (7) (`0x00417AD4`..); `AircraftClass::Mission_Enter @ 0x00419C80`
/// then idles in states 6/7 until the passenger's first cell step breaks the
/// tether (UNLOAD `8` → UNTETHER + OVER_OUT), after which `Enter_Idle_Mode`
/// re-runs without contact and lands on Guard. VERA has no transport tether;
/// the transient Enter label is not represented and the aircraft goes to
/// Guard directly. VERA-internal shortcut, gamemd end state matched.
fn aircraft_enter_idle_mode(sim: &mut Simulation, id: u64) {
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.navigation.nav_com = None;
        entity.movement_target = None;
        entity.attack_target = None;
        entity.passively_acquired_target = false;
    }
    queue_guard(sim, id);
}

/// `DAT_00817A58`: the aircraft ejection scan order, octants S, SW, SE, NW,
/// NE, N, W, E. The ninth entry is the dword that follows the table at
/// `0x00817A78` (value 4, read from the binary): the no-neighbour path leaves
/// the loop counter at 8 and `0x00415BB7` indexes the table with it.
const AIRCRAFT_EXIT_SCAN: [usize; 9] = [4, 5, 3, 7, 1, 0, 6, 2, 4];

/// AircraftClass vtable `+0x100` = `0x00415B10` (Ghidra label
/// `AircraftClass__Can_Enter_Cell` is wrong): the state-3 ejector.
///
/// `0x00415B32`..`0x00415BAB`: for each table octant, the candidate cell is
/// the aircraft cell plus `g_DirectionOffsets[octant & 7]`, kept in
/// `[ESP+0x10]`, and the passenger's `IsCellOccupied(cell, -1, -1, 0, 1)`
/// (vtable `+0x1AC`, `0x00415B8F`) ends the scan on the first zero return.
/// When every neighbour refuses, the counter stops at 8: the facing index
/// reads past the table (octant 4, S) and the destination stays the last
/// scanned cell (E). `0x00415BB1`..`0x00415BF3`: `Unlimbo(aircraft coord,
/// octant * 32)` — always the aircraft's own coordinate, whatever the scan
/// found. `InfantryClass::Unlimbo @ 0x0051DFF0` runs `PlaceInfantryInCell`
/// from that coordinate (ground Z), so the centre-quadrant `RandomRanged(0,
/// 3)` draw happens exactly when the aircraft sits within 0x3C leptons of its
/// cell centre (`0x00481180`), and a full cell fails `Unlimbo`. The caller
/// (`0x0041553E`) tests that result only to clear `passenger+0x424`
/// (`0x00415548`); nothing re-adds a failed passenger — natively it stays
/// popped from the hold (`0x00415511`) and in limbo for good, and the next
/// dispatch ejects the next passenger. VERA-internal bounded escape instead
/// (trigger: three infantry already standing in the aircraft cell, rare;
/// DRIFT — native loses the unit): the passenger goes back to the cargo head
/// (cargo departure restoration), this returns `false`, and the caller leaves Unload for
/// Guard through the empty-hold exit, so nothing retries and no further
/// epilogue draws happen. Returns `true` when the passenger left the hold. On
/// success: `Queue_Mission(Move)` (`0x00415C05`), `Set_Destination(scan
/// cell, 1)` (`0x00415C21`), then the radio handshake described on
/// [`aircraft_enter_idle_mode`]. No `LeaveTransportSound` on this path.
fn eject_from_aircraft(
    sim: &mut Simulation,
    rules: &RuleSet,
    path_grid: Option<&PathGrid>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    aircraft_id: u64,
) -> bool {
    depart_cargo_head(
        sim,
        rules,
        aircraft_id,
        DepartureRoute::LandedAircraft,
        |sim, pax_id| {
            let (cell, z, sub_x, sub_y) = match sim.substrate.entities.get(aircraft_id) {
                Some(aircraft) => (
                    (aircraft.position.rx, aircraft.position.ry),
                    aircraft.position.z,
                    aircraft.position.sub_x,
                    aircraft.position.sub_y,
                ),
                None => unreachable!("aircraft resolved before cargo departure"),
            };
            let Some(passenger_snapshot) = sim.substrate.entities.get(pax_id) else {
                return Err(DepartureFailure::MissingPassenger);
            };
            let is_infantry = passenger_snapshot.category == EntityCategory::Infantry;

            let mut index = 8usize;
            let mut scan_cell: Option<(u16, u16)> = None;
            for (i, &octant) in AIRCRAFT_EXIT_SCAN[..8].iter().enumerate() {
                let candidate = cell_from(cell, OCTANT_OFFSETS[octant & 7]);
                scan_cell = candidate;
                let passenger = sim
                    .substrate
                    .entities
                    .get(pax_id)
                    .expect("passenger resolved above");
                if candidate
                    .is_some_and(|c| passenger_can_enter(sim, rules, path_grid, passenger, c))
                {
                    index = i;
                    break;
                }
            }
            let facing = ((AIRCRAFT_EXIT_SCAN[index] & 7) as u32 * 32) as u8;

            let sub_cell = if is_infantry {
                let occupancy = sim.substrate.occupancy.get(cell.0, cell.1);
                let spot = bump_crush::allocate_sub_cell_with_preference(
                    occupancy,
                    MovementLayer::Ground,
                    None,
                    sub_x,
                    sub_y,
                    &mut sim.scenario_rng,
                );
                if spot.is_none() {
                    return Err(DepartureFailure::Placement);
                }
                spot
            } else {
                None
            };
            if let Some(passenger) = sim.substrate.entities.get_mut(pax_id) {
                passenger.sub_cell = sub_cell;
                passenger.facing = facing;
                if let Some(loco) = passenger.locomotor.as_mut() {
                    loco.layer = MovementLayer::Ground;
                }
            }
            reveal_unloaded_passenger(sim, aircraft_id, pax_id, cell.0, cell.1, z)?;
            if let Some(passenger) = sim.substrate.entities.get_mut(pax_id) {
                passenger.attack_target = None;
                passenger.passively_acquired_target = false;
                passenger.order_intent = None;
            }
            sim.queue_megamission_with_teardown(pax_id, MissionType::Move, DockTeardown::None);
            if let Some(dest) = scan_cell {
                issue_pathed_move(sim, rules, path_grid, overlay_registry, pax_id, dest);
            }
            Ok(())
        },
    )
    .is_ok()
}

#[cfg(test)]
mod tests;
