//! Transport passenger unload — the Unload mission handlers for vehicle and
//! aircraft transports (`Passengers > 0`).
//!
//! Vehicle: the `Type+0x5E0 > 0` branch of `UnitClass::Mission_Unload @
//! 0x0073D630` (vtable `+0x23C` at `0x007F5EAC`), states on `unit+0xBC`
//! through the jump table at `0x0073E5C0`, body `0x0073D70E`..`0x0073DCCE`.
//! Aircraft: `AircraftClass::Mission_Unload @ 0x004151E0` (vtable `+0x23C`
//! at `0x007E24E0`).
//!
//! Every dispatch that does not `return 10` / `return 1` early exits through
//! the common epilogue `ftol([Unload] Rate × 900) + RandomRanged(0, 2)`
//! (`0x0073E289`..`0x0073E2B5`; aircraft tail at the end of `0x004151E0`), so
//! one passenger leaves per 14..16 frames on stock `[Unload] Rate=.016`.
//!
//! Depends on `sim/`, `rules/`, `map/` only — never render/ui/audio/net.

use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_rect::{
    IsClearToMoveResult, LiveCellPassabilityQuery, evaluate_live_cell_passability,
};
use crate::sim::find_nearby_cell::PassabilityArgs;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::authority::EntityReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::bump_crush;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::passenger::{
    DepartureFailure, DepartureRoute, depart_cargo_head, reveal_unloaded_passenger,
};
use crate::sim::pathfinding::passability::LandType;
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation};
use crate::util::direction_tables::CELL_DELTAS;
use crate::util::lepton::CELL_CENTER_LEPTON;

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
/// at `0x0041541F`, inside the NavCom arm) is not ported with the rest of
/// that arm (the NavCom residual on [`mission_unload`]).
const AIR_STATE_CHECK_LANDED: u32 = 0;
const AIR_STATE_WAIT_STOP: u32 = 2;
const AIR_STATE_EJECT: u32 = 3;
const AIR_STATE_RESET: u32 = 4;

/// Early return `0xA` — "still moving / driving to land, ask again in 10".
const WAIT_MOVING_FRAMES: i32 = 10;

/// `ftol([Unload] Rate × 900) + RandomRanged(0, 2)` on the Scenario stream
/// (`Random__RandomRanged` at `0x0073E2B0` on `*(0x00A8B230)+0x218`). The
/// base is computed FIRST and consumes no RNG; the draw follows.
fn unload_epilogue(sim: &mut Simulation, rules: &RuleSet, id: u64) -> i32 {
    sim.mission_rate_epilogue_for(rules, id, MissionType::Unload)
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

fn cell_from(base: (u16, u16), delta: (i32, i32)) -> Option<(u16, u16)> {
    let x = i32::from(base.0) + delta.0;
    let y = i32::from(base.1) + delta.1;
    (x >= 0 && y >= 0 && x <= i32::from(u16::MAX) && y <= i32::from(u16::MAX))
        .then_some((x as u16, y as u16))
}

fn cell_in_map(sim: &Simulation, cell: (u16, u16)) -> bool {
    if let Some(terrain) = sim.resolved_terrain.as_ref() {
        return terrain.cell(cell.0, cell.1).is_some();
    }
    if let Some(grid) = sim.path_grid() {
        return cell.0 < grid.width() && cell.1 < grid.height();
    }
    true
}

/// Terrain level of a cell for the reveal Z, or the transport's own Z.
fn cell_level_or(sim: &Simulation, cell: (u16, u16), fallback: u8) -> u8 {
    sim.terrain_cell_level(cell.0, cell.1).unwrap_or(fallback)
}

/// The FNPC `allow_bridge_cells == 0` reject and the placement skip at
/// `0x0073D9EB` (`CellClass+0x140 & 0x100`, the structural bridge flag).
fn cell_has_structural_bridge(sim: &Simulation, cell: (u16, u16)) -> bool {
    sim.path_grid()
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
fn land_passable_for(sim: &Simulation, cell: (u16, u16), speed_type: SpeedType) -> bool {
    match sim
        .resolved_terrain
        .as_ref()
        .and_then(|terrain| terrain.cell(cell.0, cell.1))
    {
        Some(terrain_cell) => terrain_cell
            .speed_costs
            .cost_for_speed_type(speed_type)
            .is_none_or(|cost| cost > 0),
        None => sim
            .path_grid()
            .is_none_or(|grid| grid.is_walkable(cell.0, cell.1)),
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
fn octant_cell_open(sim: &Simulation, cell: (u16, u16), transport_owner: &str) -> bool {
    if !cell_in_map(sim, cell) {
        return false;
    }
    if !land_passable_for(sim, cell, SpeedType::Foot) {
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
fn pick_exit_octant(sim: &Simulation, entity: &GameEntity) -> Option<((u16, u16), usize)> {
    let owner = sim.interner.resolve(entity.owner());
    let base = (entity.position.rx, entity.position.ry);
    // `0x00740B9F..BAD`: the body's `Current()` (`+0x388`), full 16 bits.
    let facing16 = entity.body_facing_current(sim.session.binary_frame);
    let rear16 = facing16.wrapping_add(0x7FFF);
    let rear8 = i32::from((((u32::from(rear16) >> 7) + 1) >> 1) as u8 as i8);

    let mut best_score: i32 = -1;
    let mut best_octant: usize = 0;
    for (octant, &delta) in CELL_DELTAS.iter().enumerate() {
        let open = cell_from(base, delta).is_some_and(|cell| octant_cell_open(sim, cell, owner));
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
    let cell = cell_from(base, CELL_DELTAS[best_octant])?;
    Some((cell, opposite_octant(best_octant)))
}

/// State 1's wait, VERA-side: the hull turn has finished. Native state 1
/// instead tests the `+0x6AF` turret rotation latch (`0x0073D892`), which
/// Facing_Update sets only from a `Turret=` unit's `+0x3A0`
/// (`0x00736ADC..0x00736B16`), so a turretless transport moves on while its
/// hull still turns. Not yet ported.
fn hull_turn_finished(entity: &GameEntity, now: u32) -> bool {
    !entity.body_facing.is_rotating(now)
}

/// `ObjectClass::IsCellOccupied` (vtable `+0x1AC`) for the ejected passenger at
/// one cell, as called at `0x0073D994` / `0x0073D9C2` with
/// `(cell, octant, level, NULL, alt=1)`. A zero return is "may enter".
fn passenger_can_enter(
    sim: &Simulation,
    rules: &RuleSet,
    passenger: &GameEntity,
    cell: (u16, u16),
) -> bool {
    let path_grid = sim.path_grid();
    if !cell_in_map(sim, cell) {
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
    let land_passable = land_passable_for(sim, cell, speed_type);
    if path_grid.is_none() && sim.resolved_terrain.is_none() {
        // Headless fixtures have no Cell substrate: only the occupancy verdict
        // is available.
        let occupancy = sim.substrate.occupancy.get(cell.0, cell.1);
        return if passenger.category() == EntityCategory::Infantry {
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

/// The Bio Reactor's `Mission_Unload @ 0x0044D880` calls the passenger's
/// `+0x1AC` twice for each strict candidate and once for each relaxed
/// candidate. Infantry dispatches to `InfantryClass::Can_Enter_Cell
/// @ 0x0051BF90`; `0x44D988` supplies the building's `5F5F00` height,
/// octant, NULL previous cell, and mode 1. The shared `foot_can_enter` owner
/// models that active receiver (the mode is a side-effect-free locomotor
/// query); zero is the native acceptance result.
fn absorber_passenger_can_enter(
    sim: &Simulation,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    passenger_id: u64,
    cell: (u16, u16),
    octant: usize,
    height: i32,
) -> bool {
    let Some(terrain) = sim.resolved_terrain.as_ref() else {
        return false;
    };
    let cell = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain)
        .lookup((cell.0 as i16, cell.1 as i16));
    sim.foot_can_enter(
        passenger_id,
        cell,
        crate::sim::movement::infantry_entry::InfantryEntryArgs {
            direction: octant as i32,
            height,
            previous_cell: None,
        },
        rules,
        registry,
    )
    .is_ok_and(|code| code == 0)
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
/// the gunner weapon (`0x0073DC96`). A transport in a team adds the passenger
/// to it after `Set_Destination` (`0x0073DC0C..0x0073DC19`).
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
    transport_id: u64,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> EjectOutcome {
    match depart_cargo_head(
        sim,
        rules,
        overlay_registry,
        transport_id,
        DepartureRoute::Vehicle,
        |sim, pax_id| {
            let now = sim.session.binary_frame;
            let (base, facing16, transport_z, leave_sound) = {
                let transport = sim
                    .substrate
                    .entities
                    .get(transport_id)
                    .expect("transport resolved before the head pop");
                let obj = sim.object_type(transport.type_ref(), rules);
                (
                    (transport.position.rx, transport.position.ry),
                    // `0x0073D8E0..D8F4`: the hull's `Current()`, full 16 bits.
                    transport.body_facing_current(now),
                    transport.position.z,
                    obj.and_then(|o| o.leave_transport_sound.clone()),
                )
            };
            let Some(passenger_snapshot) = sim.substrate.entities.get(pax_id) else {
                // A cargo id that no longer resolves cannot be placed; keep the
                // native failure shape (re-insert) so the count stays coherent.
                return Err(DepartureFailure::MissingPassenger);
            };
            let passenger_is_infantry = passenger_snapshot.category() == EntityCategory::Infantry;

            let start =
                ((((u32::from(facing16.wrapping_add(0x7FFF))) >> 12) + 1) >> 1) as usize & 7;
            let mut strict_pass = true;
            let mut i: usize = 0;
            // `(exit cell, beyond cell on the strict pass, octant)`.
            type Placement = ((u16, u16), Option<(u16, u16)>, usize);
            let mut placement: Option<Placement> = None;
            while i < 8 {
                let octant = (start + i) & 7;
                let exit_cell = cell_from(base, CELL_DELTAS[octant]);
                let beyond_cell = exit_cell.and_then(|cell| cell_from(cell, CELL_DELTAS[octant]));
                let (adjacent_ok, beyond_ok) = match (exit_cell, beyond_cell) {
                    (Some(exit), Some(beyond)) => {
                        let passenger = sim
                            .substrate
                            .entities
                            .get(pax_id)
                            .expect("passenger resolved above");
                        (
                            passenger_can_enter(sim, rules, passenger, exit),
                            passenger_can_enter(sim, rules, passenger, beyond),
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
                if cell_has_structural_bridge(sim, exit) {
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
                // `0x0073DA96..0x0073DADD`: the passenger type's SpeedType
                // (`+0x67C`), zone -1, MovementZone Normal, seeded at the exit.
                // Residual: native Unlimbos at the null cell's coordinate when
                // nothing passes; VERA keeps the exit cell. That needs the exit
                // outside the playfield with no admitted cell within 32 rings.
                let cell = sim
                    .object_type(passenger.type_ref(), rules)
                    .and_then(|object| {
                        sim.nearby_location_cell(
                            (i32::from(exit.0), i32::from(exit.1)),
                            PassabilityArgs {
                                speed_type: object.speed_type,
                                required_zone_id: None,
                                movement_zone: MovementZone::Normal,
                                bridge_aware_zone: false,
                            },
                        )
                    })
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
                // `TechnoClass::Unlimbo` snaps the body to the drop direction
                // (`+0x388` Set_Current, `0x006F6DAA`).
                passenger.body_facing.snap((octant as u16) << 13, now);
                if let Some(loco) = passenger.locomotor.as_mut() {
                    loco.layer = MovementLayer::Ground;
                }
            }
            let z = cell_level_or(sim, place_cell, transport_z);
            reveal_unloaded_passenger(
                sim,
                rules,
                transport_id,
                pax_id,
                place_cell.0,
                place_cell.1,
                z,
                None,
                effects,
            )?;

            // OpenTopped: `TechnoClass::ClearInOpenTransport` (`0x007104A0`) drops the
            // passenger's in-transport firing membership; VERA's open-topped registry
            // is the logic-object registration the Reveal above re-establishes.
            // Leaving an OpenTopped transport of another house takes the class
            // NULL target setter (`0x0073DB9D..0x0073DBC3`); every other exit keeps
            // the passenger's target.
            let leaves_other_houses_open_transport = {
                let transport = sim
                    .substrate
                    .entities
                    .get(transport_id)
                    .expect("transport resolved before the head pop");
                sim.object_type(transport.type_ref(), rules)
                    .is_some_and(|object| object.open_topped)
                    && sim
                        .substrate
                        .entities
                        .get(pax_id)
                        .is_some_and(|passenger| passenger.owner() != transport.owner())
            };
            if leaves_other_houses_open_transport
                && let Err(cause) = sim.assign_target_represented(
                    pax_id,
                    None,
                    Some(rules),
                    overlay_registry,
                    effects,
                )
            {
                log::warn!("Unload target {pax_id}: {cause}");
            }
            // Unit73DBDB queues the passenger's Move directly, before
            // class destination73DC06; it does not execute a player event.
            sim.queue_order_mission(pax_id, MissionType::Move);
            if let Err(cause) = sim.assign_destination_represented(
                pax_id,
                Some(crate::sim::components::NavTargetRef::cell(dest.0, dest.1)),
                Some(rules),
                overlay_registry,
                effects,
            ) {
                log::warn!("Unload destination {pax_id}: {cause}");
            }
            // `0x0073DC0C..0x0073DC19`: a transport in a team takes the
            // passenger into it (`Add_Member(passenger, 0)`); boarding's
            // Conceal had removed it from its own.
            if let Some((team, _)) = sim.team_script_vm.team_for_member(transport_id) {
                sim.team_add_member(team, pax_id, false, rules, overlay_registry, effects);
            }

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
        effects,
    ) {
        Ok(()) => EjectOutcome::Placed,
        Err(_) => EjectOutcome::Failed,
    }
}

/// One `BuildingClass::Mission_Unload @ 0x0044D880` absorber attempt. Native
/// removes the cargo head before scanning, restores that same head on any
/// failed search/Unlimbo, then takes the shared Unload rate epilogue. The
/// source is the supported Steam `gamemd.exe` body `0x0044D880..0x0044DC84`;
/// infantry placement delegates to `MapClass::PlaceInfantryInCell
/// @ 0x004ACA10` and the existing `CellClass::PlaceInfantryInCell @
/// 0x00481180` owner.
///
/// Only an `InfantryAbsorb=` building reaches this, and only its infantry
/// passengers are ported. Residual: a non-infantry passenger (a custom type
/// that also sets `UnitAbsorb=`; no retail type does) fails placement and
/// stays in the cargo, as a `UnitAbsorb=`-only building's do.
pub(crate) fn building_mission_unload_absorber(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> i32 {
    let now = sim.session.binary_frame;
    let Some((base, facing16, building_owner, extra_power, height)) =
        sim.substrate.entities.get(id).and_then(|building| {
            let object = sim.object_type(building.type_ref(), rules)?;
            // Native 0x44D92D calls Building+0x1B8 (GetMapCoords) and scans
            // outward from its Location cell. The sound tail separately uses
            // +0x48 GetCoords (the foundation center).
            let base = (building.position.rx, building.position.ry);
            let height = sim
                .resolved_terrain
                .as_ref()
                .map(|terrain| {
                    let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
                    crate::sim::movement::ground_pose::query_object_cell_height(
                        &cells,
                        crate::sim::movement::ground_pose::position_world_coord(&building.position),
                        building.on_bridge,
                    )
                })
                .unwrap_or(0);
            Some((
                base,
                building.body_facing_current(now),
                building.owner(),
                // Native Type+0xEE8 is `ExtraPower`: BuildingType::ReadINI
                // passes the raw `ExtraPower` string at 0x81A7B0 to the
                // integer reader at 0x5276D0 and stores it at +0xEE8
                // (0x004610B3..0x004610C7). The unload tail tests this field
                // `> 0` before dirtying House+0x5778 (0x0044DC55..0x0044DC75).
                object.extra_power > 0,
                height,
            ))
        })
    else {
        return unload_epilogue(sim, rules, id);
    };

    let _ = depart_cargo_head(
        sim,
        rules,
        overlay_registry,
        id,
        DepartureRoute::Absorber,
        |sim, passenger_id| {
            let Some(passenger_category) = sim
                .substrate
                .entities
                .get(passenger_id)
                .map(|passenger| passenger.category())
            else {
                return Err(DepartureFailure::MissingPassenger);
            };
            if passenger_category != EntityCategory::Infantry {
                return Err(DepartureFailure::Placement);
            }
            let start =
                ((((u32::from(facing16.wrapping_add(0x7FFF)) >> 12) + 1) >> 1) as usize) & 7;
            let mut strict_pass = true;
            let mut index = 0usize;
            let mut placement = None;
            while index < 8 {
                let octant = (start + index) & 7;
                let adjacent = cell_from(base, CELL_DELTAS[octant]);
                let beyond = adjacent.and_then(|cell| cell_from(cell, CELL_DELTAS[octant]));
                let accepts = if strict_pass {
                    // Native tests the cell two steps away before the adjacent
                    // cell. The relaxed pass calls only the adjacent cell.
                    let beyond_open = beyond.is_some_and(|cell| {
                        absorber_passenger_can_enter(
                            sim,
                            rules,
                            overlay_registry,
                            passenger_id,
                            cell,
                            octant,
                            height,
                        )
                    });
                    beyond_open
                        && adjacent.is_some_and(|cell| {
                            absorber_passenger_can_enter(
                                sim,
                                rules,
                                overlay_registry,
                                passenger_id,
                                cell,
                                octant,
                                height,
                            )
                        })
                } else {
                    adjacent.is_some_and(|cell| {
                        absorber_passenger_can_enter(
                            sim,
                            rules,
                            overlay_registry,
                            passenger_id,
                            cell,
                            octant,
                            height,
                        )
                    })
                };
                if !accepts {
                    if strict_pass && index == 7 {
                        strict_pass = false;
                        index = 1;
                    } else {
                        index += 1;
                    }
                    continue;
                }
                let adjacent = adjacent.expect("accepted adjacent cell exists");
                // The native structural bridge skip advances the octant; it
                // does not trigger the relaxed-pass transition at index 7.
                if cell_has_structural_bridge(sim, adjacent) {
                    index += 1;
                    continue;
                }
                let strict_beyond = if strict_pass { beyond } else { None };
                placement = Some((adjacent, strict_beyond, octant));
                break;
            }
            let Some((adjacent, strict_beyond, octant)) = placement else {
                return Err(DepartureFailure::Placement);
            };

            // Native supplies the cell centre (x*256+0x80,y*256+0x80,z=0)
            // to 4ACA10. A bridge candidate was skipped above, so this path
            // uses the selected ground plane and consumes the shared
            // center-row Scenario draw in 481180.
            let Some(sub_cell) = bump_crush::place_infantry_in_cell(
                &sim.substrate.raw_cell_occupation,
                adjacent.0,
                adjacent.1,
                MovementLayer::Ground,
                CELL_CENTER_LEPTON,
                CELL_CENTER_LEPTON,
                &mut sim.scenario_rng,
            ) else {
                return Err(DepartureFailure::Placement);
            };
            let destination = strict_beyond.unwrap_or(adjacent);
            if let Some(passenger) = sim.substrate.entities.get_mut(passenger_id) {
                passenger.sub_cell = Some(sub_cell);
                passenger.body_facing.snap((octant as u16) << 13, now);
                if let Some(locomotor) = passenger.locomotor.as_mut() {
                    locomotor.layer = MovementLayer::Ground;
                }
            }
            let fallback_z = sim
                .substrate
                .entities
                .get(id)
                .map_or(0, |building| building.position.z);
            let z = cell_level_or(sim, adjacent, fallback_z);
            reveal_unloaded_passenger(
                sim,
                rules,
                id,
                passenger_id,
                adjacent.0,
                adjacent.1,
                z,
                Some((octant as u8) << 5),
                effects,
            )?;

            if let Some(sound) = rules.general.leave_bio_reactor_sound.as_ref() {
                let coordinate = sim.substrate.entities.get(id).map(|building| {
                    crate::sim::movement::ground_pose::object_get_coords(
                        building,
                        sim.resolved_terrain.as_ref(),
                    )
                });
                if let Some(coordinate) = coordinate {
                    let (rx, ry, sub_x, sub_y, _) = crate::sim::anim_class::AnimWorldCoord {
                        x: coordinate.x,
                        y: coordinate.y,
                        z: coordinate.z,
                    }
                    .to_cell_sub_z();
                    sim.sound_events.push(SimSoundEvent::VocAt {
                        sound_id: sound.clone(),
                        audible_to: None,
                        rx,
                        ry,
                        sub_x,
                        sub_y,
                        world_z_leptons: coordinate.z,
                    });
                }
            }
            sim.clear_human_absorber_seek_on_unload(passenger_id);
            sim.restore_absorbed_infantry_tracking(passenger_id);

            sim.queue_order_mission(passenger_id, MissionType::Move);
            if let Err(cause) = sim.assign_destination_represented(
                passenger_id,
                Some(crate::sim::components::NavTargetRef::cell(
                    destination.0,
                    destination.1,
                )),
                Some(rules),
                overlay_registry,
                effects,
            ) {
                log::warn!("Building Unload destination {passenger_id}: {cause}");
            }
            if extra_power {
                sim.invalidate_house_power(building_owner, false);
            }
            Ok(())
        },
        effects,
    );
    unload_epilogue(sim, rules, id)
}

/// The `Passengers > 0` branch of `UnitClass::Mission_Unload @ 0x0073D630`.
///
/// Returns the handler's dispatch delay; the caller writes the epilogue.
pub(crate) fn unit_mission_unload(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> i32 {
    let now = sim.session.binary_frame;
    let Some(entity) = sim.substrate.entities.get(id) else {
        return unload_epilogue(sim, rules, id);
    };
    match entity.mission.handler_state() {
        STATE_PICK_EXIT => {
            // `ILocomotion::Is_Moving` (`0x0073D729`) → `return 10`. Neither
            // an order nor Is_Moving_Now is that query.
            if crate::sim::movement::motion_query::is_moving(entity) == Some(true) {
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
                // Zone -1, MovementZone Normal, seeded at the transport's own
                // cell (`0x0073D772..0x0073D7B0`). Residual: when nothing
                // passes, native still sets the destination to the null cell
                // (`0x0073D7B5..0x0073D7D1`); VERA sets none and waits. That
                // needs no Wheel ground within 32 rings of the transport.
                let seed = (i32::from(entity.position.rx), i32::from(entity.position.ry));
                let wheel_land = PassabilityArgs {
                    speed_type: SpeedType::Wheel,
                    required_zone_id: None,
                    movement_zone: MovementZone::Normal,
                    bridge_aware_zone: false,
                };
                if let Some(cell) = sim.nearby_location_cell(seed, wheel_land) {
                    // `Set_Destination` only — the committed selector stays
                    // Unload and state 0 re-runs once the drive has ended.
                    if let Err(cause) = sim.assign_destination_represented(
                        id,
                        Some(crate::sim::components::NavTargetRef::cell(cell.0, cell.1)),
                        Some(rules),
                        overlay_registry,
                        effects,
                    ) {
                        log::warn!("Unload destination {id}: {cause}");
                    }
                }
                return WAIT_MOVING_FRAMES;
            }
            let cargo = entity.passenger_role.passenger_count();
            let pick = pick_exit_octant(sim, entity);
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
                    //73D86C dispatches the active ILocomotion+4C owner.
                    crate::sim::movement::locomotor_do_turn(
                        entity,
                        (face_octant as u16) << 13,
                        now,
                    );
                    entity.mission.set_handler_state(STATE_TURNING);
                }
                return 1;
            }
            // Nothing to unload or nowhere to unload: `Queue_Mission(Guard)`
            // (`0x0073D887`) and the epilogue.
            queue_guard(sim, id);
            unload_epilogue(sim, rules, id)
        }
        STATE_TURNING => {
            if hull_turn_finished(entity, now) {
                if let Some(entity) = sim.substrate.entities.get_mut(id) {
                    entity.mission.set_handler_state(STATE_EJECT);
                }
                return 1;
            }
            unload_epilogue(sim, rules, id)
        }
        STATE_EJECT => {
            if entity.passenger_role.passenger_count() > entity.transport_unload_keep_count {
                let _ = eject_head_passenger(sim, rules, id, overlay_registry, effects);
            } else if let Some(entity) = sim.substrate.entities.get_mut(id) {
                entity.mission.set_handler_state(STATE_DONE);
            }
            unload_epilogue(sim, rules, id)
        }
        STATE_DONE => {
            // `Queue_Mission(Guard)` then `+0xB8 = 1` (`0x0073DCC1`..`0x0073DCC7`).
            queue_guard(sim, id);
            if let Some(entity) = sim.substrate.entities.get_mut(id) {
                entity.mission.set_movement_bypass_latch();
            }
            unload_epilogue(sim, rules, id)
        }
        _ => unload_epilogue(sim, rules, id),
    }
}

/// Whether the aircraft is on the ground: `GetHeight() == 0` (vtable `+0x1C8`
/// at `0x004151FF`) and the Fly approach pitch `+0x2E8 == 0.0` (`fld` at
/// `0x0041520D`, compared through `0x0041521E`).
fn aircraft_landed(
    entity: &GameEntity,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> bool {
    crate::sim::movement::air_movement::current_fly_height(entity, terrain) == 0
        && entity.flight_attitude.is_level()
}

/// The Aircraft Unload slot `0x004151E0`, for a `Passengers > 0` aircraft;
/// returns its frames ([`crate::sim::aircraft::dispatch_mission`]
/// gates it on the mission timer and restarts the timer). Dormant in retail: read
/// through `RuleSet::from_ini`, no `[AircraftTypes]` entry has `Passengers=`.
/// The Nighthawk `[SHAD]` is a Jumpjet `[VehicleTypes]` entry and unloads
/// through [`unit_mission_unload`].
///
/// The arms without a NavCom (`+0x5A4 == NULL`; the NavCom arms are a
/// residual below):
///
/// - State 0 (`0x004151FB`): `GetHeight() == 0` (`0x004151FF`) and
///   `+0x2E8 == 0.0` (`0x0041520D`), then the NavCom test (`0x00415220`):
///   none → state 3 (`0x00415250`). Not landed → `0x00415290`: the airfield
///   branch needs `Type+0xC95` = `IsDropship=` (`TechnoTypeClass::ReadINI`
///   `0x00712350`..`0x00712373`, key string `0x0084447C`; `AirportBound=` is
///   a different field, `AircraftType+0xE0D`, `0x0041CC6E`), and no retail
///   rulesmd.ini type sets `IsDropship`, so it takes state 2 (`0x0041530C`).
///   Both arms fall into the epilogue at `0x0041525A` (one
///   `RandomRanged(0, 2)` draw).
/// - State 3 early-out: `+0x418 != 0` returns before the hold test
///   (`0x004154BB`..`0x004154C3`); its writer and meaning are UNCHECKED and
///   VERA does not represent it.
/// - States 1 and 4 are written at `0x0041541F`, inside the NavCom arm
///   (residual below).
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
///
/// The empty hold's idle mode (`vt+0x484(0, 1)` at `0x004155B5`, the hold
/// already empty on state-3 entry, and `0x004155A2`, the ejection just
/// emptied it) is `aircraft::idle_entry`'s.
///
/// RESIDUAL: the transport tether. The last ejection leaves the aircraft in
/// radio contact with the passenger (`Transmit_Message(HELLO)` `0x00415C2E`,
/// then `UNLOADED` `0x00415C3F`, which `TechnoClass::Receive_Radio @
/// 0x006F4AB0` answers with TETHER `0x18`), so `In_Radio_Contact` steers the
/// first idle pass to Enter (7); `AircraftClass::Mission_Enter @ 0x00419C80`
/// then idles in states 6/7 until the passenger's first cell step breaks the
/// tether (UNLOAD `8` -> UNTETHER + OVER_OUT), after which Enter_Idle_Mode
/// re-runs without contact and lands on Guard. VERA keeps no tether, so the
/// aircraft takes Guard at once. Dormant in retail (no aircraft carries
/// passengers, above).
///
/// RESIDUAL: the NavCom arms are not ported. A landed aircraft compares its
/// NavCom with its own coordinate (`0x0041522A..0x0041524E`) and takes state
/// 3 only on a match. Otherwise, with a NavCom, `0x0041531B..0x00415425`
/// asks vt+0x550 about it and then sets a new destination, with a team call
/// on `+0x5D4` at `0x004153DC`, or writes state 1 or 4. Trigger: an aircraft
/// transport ordered to unload while it has a NavCom. Dormant in retail
/// (above).
///
/// RESIDUAL: the team join of each passenger ejected here
/// (`0x0041556A..0x00415587`) is not ported; the vehicle arm's
/// (`0x0073DC0C..0x0073DC19`) is. Unlike that arm it is gated on the
/// passenger's own team (`+0x5D4` at `0x00415574`) and its `+0x424` byte
/// before the aircraft's team adds it (`TeamClass::Add_Member`), so a
/// passenger that left its team as it boarded is not added either. Trigger:
/// an aircraft transport unloads a passenger that kept a team. Effect: it
/// is not added to the aircraft's team. Dormant in retail (above).
pub(crate) fn mission_unload(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> i32 {
    let entity = sim.substrate.entities.get(id).expect("dispatched aircraft");
    match entity.mission.handler_state() {
        AIR_STATE_CHECK_LANDED => {
            // Without a NavCom: landed → 3 (`0x00415250`), otherwise → 2
            // (`0x0041530C`). The NavCom arms are a residual above.
            let next = if aircraft_landed(entity, sim.resolved_terrain.as_ref()) {
                AIR_STATE_EJECT
            } else {
                AIR_STATE_WAIT_STOP
            };
            if let Some(entity) = sim.substrate.entities.get_mut(id) {
                entity.mission.set_handler_state(next);
            }
            unload_epilogue(sim, rules, id)
        }
        AIR_STATE_WAIT_STOP => {
            // `Is_Moving` false (`0x0041549D`) → state 3.
            //
            // RESIDUAL (VERA-only): the landed gate has no native counterpart
            // here; native state 2 asks only Is_Moving. It stays because
            // `eject_from_aircraft` reveals each passenger at the aircraft's
            // own height (the landed-aircraft route), so a still aircraft at
            // altitude would leave its cargo in the air. Dormant in retail
            // (see the doc above).
            if crate::sim::movement::motion_query::is_moving(entity) != Some(true)
                && aircraft_landed(entity, sim.resolved_terrain.as_ref())
                && let Some(entity) = sim.substrate.entities.get_mut(id)
            {
                entity.mission.set_handler_state(AIR_STATE_EJECT);
            }
            1
        }
        AIR_STATE_EJECT => {
            if entity.passenger_role.passenger_count() == 0 {
                crate::sim::aircraft::enter_idle_mode_for(
                    sim,
                    id,
                    rules,
                    overlay_registry,
                    effects,
                );
            } else {
                let ejected = eject_from_aircraft(sim, rules, overlay_registry, id, effects);
                let hold_empty = sim
                    .substrate
                    .entities
                    .get(id)
                    .is_some_and(|entity| entity.passenger_role.passenger_count() == 0);
                // `!ejected`: VERA's bounded escape from a refused ejection
                // (native loses the passenger and keeps ejecting the rest;
                // see [`eject_from_aircraft`]). The hold keeps the passenger
                // and the mission leaves through the same empty-hold exit, so
                // this dispatch's epilogue draw is the last one.
                if hold_empty || !ejected {
                    crate::sim::aircraft::enter_idle_mode_for(
                        sim,
                        id,
                        rules,
                        overlay_registry,
                        effects,
                    );
                }
            }
            unload_epilogue(sim, rules, id)
        }
        AIR_STATE_RESET => {
            if let Some(entity) = sim.substrate.entities.get_mut(id) {
                entity.mission.set_handler_state(AIR_STATE_CHECK_LANDED);
            }
            1
        }
        _ => unload_epilogue(sim, rules, id),
    }
}

/// `DAT_00817A58`: the aircraft ejection scan order, octants S, SW, SE, NW,
/// NE, N, W, E. The ninth entry is the dword that follows the table at
/// `0x00817A78` (value 4, read from the binary): the no-neighbour path leaves
/// the loop counter at 8 and `0x00415BB7` indexes the table with it.
const AIRCRAFT_EXIT_SCAN: [usize; 9] = [4, 5, 3, 7, 1, 0, 6, 2, 4];

/// AircraftClass vtable `+0x100` = `0x00415B10` (Ghidra label
/// `AircraftClass__Can_Enter_Cell` is wrong): the state-3 ejector, run between
/// the dispatcher's `Pick_Up` and `Place_Down` of the aircraft
/// (`0x0041552E` / `0x00415565`). Mark lists a landed Jumpjet transport in
/// its Cell with its 0x40, which would otherwise refuse the passenger's
/// placement there.
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
/// tether residual on [`mission_unload`]'s idle mode. No `LeaveTransportSound`
/// on this path.
fn eject_from_aircraft(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    aircraft_id: u64,
    effects: FrameEffects<'_>,
) -> bool {
    let mut picked_up = false;
    let ejected = depart_cargo_head(
        sim,
        rules,
        overlay_registry,
        aircraft_id,
        DepartureRoute::LandedAircraft,
        |sim, pax_id| {
            // `0x0041552E`: with the head popped, the aircraft leaves its Cell
            // (`MapClass::Pick_Up`), taking its list entry and raw bit along.
            sim.foot_pick_up(aircraft_id, Some(rules), overlay_registry);
            picked_up = true;
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
            let is_infantry = passenger_snapshot.category() == EntityCategory::Infantry;

            let mut index = 8usize;
            let mut scan_cell: Option<(u16, u16)> = None;
            for (i, &octant) in AIRCRAFT_EXIT_SCAN[..8].iter().enumerate() {
                let candidate = cell_from(cell, CELL_DELTAS[octant & 7]);
                scan_cell = candidate;
                let passenger = sim
                    .substrate
                    .entities
                    .get(pax_id)
                    .expect("passenger resolved above");
                if candidate.is_some_and(|c| passenger_can_enter(sim, rules, passenger, c)) {
                    index = i;
                    break;
                }
            }
            let facing = ((AIRCRAFT_EXIT_SCAN[index] & 7) as u16) << 13;

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
            let now = sim.session.binary_frame;
            if let Some(passenger) = sim.substrate.entities.get_mut(pax_id) {
                passenger.sub_cell = sub_cell;
                // `Unlimbo`'s body snap (`0x006F6DAA`).
                passenger.body_facing.snap(facing, now);
                if let Some(loco) = passenger.locomotor.as_mut() {
                    loco.layer = MovementLayer::Ground;
                }
            }
            reveal_unloaded_passenger(
                sim,
                rules,
                aircraft_id,
                pax_id,
                cell.0,
                cell.1,
                z,
                None,
                effects,
            )?;
            // KickOutUnit leaves the passenger's target alone
            // (`0x00415BF9..0x00415C21`).
            // Aircraft415C05 queues Move directly before destination415C21,
            // followed by its radio callbacks. No Event Archive clear.
            sim.queue_order_mission(pax_id, MissionType::Move);
            if let Some(dest) = scan_cell
                && let Err(cause) = sim.assign_destination_represented(
                    pax_id,
                    Some(crate::sim::components::NavTargetRef::cell(dest.0, dest.1)),
                    Some(rules),
                    overlay_registry,
                    effects,
                )
            {
                log::warn!("Unload destination {pax_id}: {cause}");
            }
            Ok(())
        },
        effects,
    )
    .is_ok();
    // `0x00415565`: and goes back once the ejector returns.
    if picked_up {
        sim.foot_place_down(aircraft_id, Some(rules), overlay_registry, &mut |_, _| {});
    }
    ejected
}

#[cfg(test)]
mod tests;
