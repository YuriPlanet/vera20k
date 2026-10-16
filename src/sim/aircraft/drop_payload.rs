//! `AircraftClass::Drop_Payload @ 0x00415C60`: one passenger out of the
//! paradrop plane, each visit of Mission_ParadropOverfly within the radius
//! ([`super::paradrop_mission`]), and out of any aircraft whose Fire_At finds
//! a passenger aboard ([`super::fire_at`]).
//!
//! In order:
//! - The cargo's head leaves (`CargoClass::RemoveFirstPassenger @
//!   0x00473430`); with none, nothing happens. Ammo (`+0x2FC`) drops by one.
//! - The landing point is half a cell to the side of the plane's coordinate
//!   (GetCoords, vt+0x48): its facing turned a quarter left for an odd Ammo,
//!   right for an even one ([`drop_world_xy`]).
//! - The passenger's `Can_Enter_Cell` (vt+0x1AC, Infantry `0x0051BF90`) on
//!   that cell, with direction -1, height -1, no previous cell and mode 1,
//!   must answer 0 (`0x00415D7D..0x00415D95`).
//! - `CellClass::PlaceInfantryInCell @ 0x00481180` picks a ground spot
//!   there, with no priority (`0x00415D9B..0x00415DD1`); a full cell refuses.
//! - SpawnParachuted (vt+0xE8, [`spawn_parachuted`]) at the spot's XY and
//!   the plane's Z (`0x00415DD7..0x00415DF0`).
//! - Success: ChuteSound (`[AudioVisual] ChuteSound=`, `Rules+0x71C`) plays
//!   at the plane's Location (`0x00415DF6..0x00415E21`), the passenger's
//!   neighbour cell (`+0x55C`) becomes the landing cell
//!   (`0x00415E26..0x00415E6E`), it leaves the plane's team
//!   (`TeamClass::Remove @ 0x006EA870`, `0x00415E74..0x00415E83`), and the
//!   plane gets its five passes back (`+0x6D3`) and restarts its rearm timer
//!   (`+0x2EC`) with no time (`0x00415E88..0x00415EAA`).
//! - Failure (`0x00415EB1..0x00415EC7`): the passenger boards again at the
//!   head (`CargoClass::AddPassenger @ 0x004733A0`), forgets its discovery
//!   ([`Simulation::techno_clear_discovery_on_conceal`], vt+0x11C) and Ammo
//!   gets its point back. The next Overfly visit tries again.
//!
//! The landing coordinate keeps the original's arithmetic: the full facing
//! word, the ±0x3FFF wrap and the truncation after adding the table
//! displacement to world XY (`native_trig` owns that math).
//!
//! Evidence: `tools/superweapon_oracle.py` sections `drop_payload` and
//! `spawn_parachuted` run Drop_Payload and SpawnParachuted;
//! `superweapon/paradrop_tests.rs` replays them. The landing coordinates
//! over every facing: `tools/spatial_oracle/paradrop_coordinates.*`.
//!
//! RESIDUALS:
//! - ObjectClass::Paradrop raises the falling byte (`+0x8D`, `0x005F5965`)
//!   before its checks and keeps it when one refuses; VERA starts the fall
//!   once they pass. A refused passenger rides on in limbo, where no
//!   gameplay code reads the byte (ComputeCRC `0x005F62FA` hashes it), and
//!   its next drop raises it again: the same answer.
//! - Paradrop compares the landing cell's zone (`MapClass @ 0x0056D230`
//!   with OnBridge) with itself in IsClearToMove; VERA asks for no zone, the
//!   same answer.
//!
//! Ledger:
//! - Scenario draws: PlaceInfantryInCell's row draw for a centre or
//!   north-west spot, and the Unlimbo's.
//! - Timer writes: the plane's rearm timer (`+0x2EC`) on success.
//! - Detach calls: none.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, map/, util/native_trig, sim/cell_rect,
//!   sim/movement, sim/passenger, sim/world.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::map::bridge_facts::{BRIDGE_FLAG_STRUCTURAL, BRIDGE_FLAG_TRANSITION};
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_rect::{
    IsClearToMoveResult, LiveCellPassabilityQuery, cell_is_in_playfield_leptons,
    evaluate_live_cell_passability,
};
use crate::sim::components::DriveCoord;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::infantry_action::DO_PARADROP;
use crate::sim::movement::infantry_entry::InfantryEntryArgs;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::movement::parachute_descent::begin_parachute_descent;
use crate::sim::movement::{ground_pose, walk_head};
#[cfg(any(test, feature = "test-support"))]
use crate::sim::passenger::PassengerRole;
use crate::sim::passenger::{DepartureFailure, DepartureRoute, depart_cargo_head};
use crate::sim::world::{
    FrameEffects, PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest, SimSoundEvent,
    Simulation, UninitContext,
};
use crate::util::native_trig::facing_step_world_xy;

/// The landing point's distance from the plane's coordinate (`0x007E2808`,
/// 128.0 leptons, half a cell).
const V_PATTERN_RADIUS_LEPTONS: i32 = 128;

/// Unlimbo's direction for a paradropped object (`PUSH 0x80` at
/// `0x005F5A35`): it lands facing south.
const PARADROP_UNLIMBO_FACING: u8 = 0x80;

/// What [`drop_payload`] called, in the oracle's terms (observation only).
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Observed {
    /// Map[coord] (`0x00565730`) for the landing point, then for the spot.
    CellAt([i32; 3]),
    /// The passenger's Can_Enter_Cell and the plane's Ammo at the call.
    CanEnter(u64, i32),
    /// PlaceInfantryInCell's input coordinate.
    Subposition([i32; 3]),
    /// SpawnParachuted's coordinate.
    SpawnParachuted(u64, [i32; 3]),
    /// SpawnParachuted's Queue_Mission (vt+0x1E8) for the placed passenger.
    Queue(u64, MissionId),
    /// SpawnParachuted's Do_Action (vt+0x558): the action and its force.
    DoAction(u64, i32, bool),
    /// ChuteSound's coordinate.
    PlayAt([i32; 3]),
    /// The plane's team let the passenger go.
    RemoveMember(u64),
    /// A refused passenger boarded again (AddPassenger's Limbo), then vt+0x11C.
    Limbo(u64),
    Conceal(u64),
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// Observation only: what [`drop_payload`] called on this thread while a
    /// test holds `Some`.
    pub(crate) static OBSERVED: std::cell::RefCell<Option<Vec<Observed>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(any(test, feature = "test-support"))]
fn observe(call: Observed) {
    OBSERVED.with(|log| {
        if let Some(log) = log.borrow_mut().as_mut() {
            log.push(call);
        }
    });
}

/// The landing point's world XY, in whole leptons, from the plane's
/// coordinate, its body facing word (`FacingClass::Current @ 0x004C93D0` of
/// `+0x388`) and its Ammo after the decrement.
///
/// `0x00415CA6..0x00415D78`: an odd Ammo (its low byte's bit 0) turns the
/// facing 0x3FFF left, an even one 0x3FFF right, then the same signed-angle
/// table step as Walk and Fly moves 128 leptons that way. Rounding a separate
/// offset would lose the original's final truncation and can change the cell.
/// Native corpus: `tools/spatial_oracle/paradrop_coordinates.{py,json,md}`.
fn drop_world_xy(current: [i32; 2], facing: u16, ammo: i32) -> [i32; 2] {
    let drop_facing = if ammo & 1 == 0 {
        facing.wrapping_add(0x3FFF)
    } else {
        facing.wrapping_sub(0x3FFF)
    };
    facing_step_world_xy(current, drop_facing, V_PATTERN_RADIUS_LEPTONS)
}

/// `AircraftClass::Drop_Payload @ 0x00415C60` for plane `id`: see the module
/// doc.
pub(crate) fn drop_payload(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    let mut head = None;
    let result = depart_cargo_head(
        sim,
        rules,
        registry,
        id,
        DepartureRoute::Paradrop,
        |sim, passenger| {
            head = Some(passenger);
            let ammo = add_ammo(sim, id, -1);
            let landing = drop_passenger(sim, rules, registry, id, passenger, ammo, effects)?;
            land_tail(sim, rules, id, passenger, landing, effects);
            Ok(())
        },
        effects,
    );
    if let (Err(failure), Some(passenger)) = (result, head)
        && failure != DepartureFailure::NoCargo
    {
        #[cfg(any(test, feature = "test-support"))]
        {
            observe(Observed::Limbo(passenger));
            observe(Observed::Conceal(passenger));
        }
        sim.techno_clear_discovery_on_conceal(passenger);
        add_ammo(sim, id, 1);
    }
}

/// Adds `delta` to the plane's Ammo (`+0x2FC`, a signed int that wraps) and
/// returns the new count. `AircraftClass::Drop_Payload` (`0x00415C60`) takes
/// one at `0x00415C7D..0x00415C8D` and gives it back at `0x00415EC7`.
fn add_ammo(sim: &mut Simulation, id: u64, delta: i32) -> i32 {
    sim.substrate
        .entities
        .get_mut(id)
        .and_then(|plane| plane.ammo_mut())
        .map_or(0, |ammo| ammo.add(delta))
}

/// From the landing point through SpawnParachuted
/// (`0x00415C93..0x00415DF0`). Returns the coordinate the passenger landed
/// at.
fn drop_passenger(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    plane: u64,
    passenger: u64,
    ammo: i32,
    effects: FrameEffects<'_>,
) -> Result<DriveCoord, DepartureFailure> {
    match sim.substrate.entities.get(passenger) {
        None => return Err(DepartureFailure::MissingPassenger),
        Some(rider) if !(rider.lifecycle.object_alive() && rider.lifecycle.in_limbo()) => {
            return Err(DepartureFailure::NotReady);
        }
        Some(_) => {}
    }
    let frame = sim.session.binary_frame;
    let terrain = sim.resolved_terrain.as_ref();
    let Some(plane_entity) = sim.substrate.entities.get(plane) else {
        return Err(DepartureFailure::Placement);
    };
    // GetCoords (`0x00415C93`): the landing point keeps the plane's Z.
    let [x, y] = drop_world_xy(
        ground_pose::position_world_xy(&plane_entity.position),
        plane_entity.body_facing_current(frame),
        ammo,
    );
    let z = ground_pose::object_world_z_leptons(plane_entity, terrain);
    let point = DriveCoord { x, y, z };
    #[cfg(any(test, feature = "test-support"))]
    observe(Observed::CellAt([x, y, z]));
    let cell = terrain.map(|terrain| NativeCellQuery::canonical(terrain).lookup_world(x, y));

    // Headless fixtures have no map cells to ask.
    if let Some(cell) = cell {
        #[cfg(any(test, feature = "test-support"))]
        observe(Observed::CanEnter(passenger, ammo));
        match sim.foot_can_enter(passenger, cell, InfantryEntryArgs::REPAIR, rules, registry) {
            Ok(0) => {}
            Ok(_) => return Err(DepartureFailure::Placement),
            Err(cause) => {
                log::debug!("paradrop passenger {passenger} Can_Enter_Cell: {cause}");
                return Err(DepartureFailure::Placement);
            }
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    observe(Observed::Subposition([x, y, z]));
    let Some(spot) = sim.place_infantry_in_ground_cell(rules, point) else {
        return Err(DepartureFailure::Placement);
    };
    let spot_xy = walk_head::selected_head(point, spot, z, false);
    let landing = DriveCoord {
        x: spot_xy.x,
        y: spot_xy.y,
        z,
    };
    spawn_parachuted(sim, rules, registry, passenger, landing, spot, effects)?;
    Ok(landing)
}

/// `InfantryClass::SpawnParachuted @ 0x00521760` (vt+0xE8) at `landing`:
/// ObjectClass::Paradrop, then, once it placed the passenger, Guard for a
/// house controlled by a human, else Hunt (`HouseClass::IsControlledByHuman
/// @ 0x0050B730`, Queue_Mission at `0x00521788`/`0x00521798`), and a forced
/// Paradrop action (Do_Action `0x21`, `0x005217A8`).
fn spawn_parachuted(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    passenger: u64,
    landing: DriveCoord,
    spot: u8,
    effects: FrameEffects<'_>,
) -> Result<(), DepartureFailure> {
    #[cfg(any(test, feature = "test-support"))]
    observe(Observed::SpawnParachuted(
        passenger,
        [landing.x, landing.y, landing.z],
    ));
    paradrop(sim, rules, registry, passenger, landing, spot, effects)?;
    let Some(rider) = sim.substrate.entities.get(passenger) else {
        return Ok(());
    };
    if rider.category() != EntityCategory::Infantry {
        return Ok(());
    }
    let human = sim.owner_is_human(rider.owner());
    let mission = MissionId::from_known(if human {
        MissionType::Guard
    } else {
        MissionType::Hunt
    });
    #[cfg(any(test, feature = "test-support"))]
    observe(Observed::Queue(passenger, mission));
    if let Some(rider) = sim.substrate.entities.get_mut(passenger) {
        crate::sim::mission::authority::queue_entity_mission_deferred(rider, mission);
    }
    #[cfg(any(test, feature = "test-support"))]
    observe(Observed::DoAction(passenger, DO_PARADROP, true));
    if let Err(cause) = sim.infantry_do_action(passenger, DO_PARADROP, true, rules, effects) {
        log::debug!("paradrop passenger {passenger} Do_Action: {cause}");
    }
    Ok(())
}

/// `ObjectClass::Paradrop @ 0x005F5940` at `landing`, in order:
/// - the coordinate over the playfield (`MapClass::IsCoordInPlayfield @
///   0x005785F0`);
/// - a bridge cell (`+0x140 & 0x100`) sets OnBridge (`0x005F5986`), which
///   no refusal clears, and needs the `0x200` flag too
///   (`0x005F598D..0x005F5996`);
/// - a Techno needs IsClearToMove (`0x004834A0`) for its SpeedType and
///   MovementZone, any level (`0x005F599C..0x005F5A2D`);
/// - Unlimbo (vt+0xD8) at the coordinate facing south, then SetLocation
///   (vt+0x1B4), which the falling Z keeps ([`begin_parachute_descent`]);
/// - the canopy ([`Simulation::attach_parachute_anim`]).
///
/// A passenger refused over a bridge cell thus boards again with OnBridge
/// set. Its next drop, over ground too, falls to the deck's height (GetHeight
/// `0x005F5F40`), and it walks there until it steps off a bridge cell
/// (`0x0075C180..0x0075C193`) or boards a transport (`0x0051A407`).
fn paradrop(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    passenger: u64,
    landing: DriveCoord,
    spot: u8,
    effects: FrameEffects<'_>,
) -> Result<(), DepartureFailure> {
    if !cell_is_in_playfield_leptons(
        (landing.x, landing.y, landing.z),
        sim.playfield_bounds,
        sim.resolved_terrain.as_ref(),
    ) {
        return Err(DepartureFailure::Placement);
    }
    let mut position = sim
        .substrate
        .entities
        .get(passenger)
        .map(|rider| rider.position)
        .ok_or(DepartureFailure::MissingPassenger)?;
    position.put_world_xy([landing.x, landing.y]);
    let (rx, ry) = (position.rx(), position.ry());
    let flags = sim.resolved_terrain.as_ref().map_or(0, |terrain| {
        terrain.native_cell_flags(
            NativeCellQuery::canonical(terrain).lookup_world(landing.x, landing.y),
        )
    });
    // OnBridge goes up before the tests below and stays up: neither a refusal
    // here nor Drop_Payload's AddPassenger lowers it, so a passenger refused
    // over a bridge drops later as if onto a deck. Its locomotor layer is the
    // OnBridge projection.
    if flags & BRIDGE_FLAG_STRUCTURAL != 0
        && let Some(rider) = sim.substrate.entities.get_mut(passenger)
    {
        rider.on_bridge.store(true);
        if let Some(locomotor) = rider.locomotor.as_mut() {
            locomotor.store_ground_layer(true);
        }
    }
    if flags & BRIDGE_FLAG_STRUCTURAL != 0 && flags & BRIDGE_FLAG_TRANSITION == 0 {
        return Err(DepartureFailure::Placement);
    }

    let (speed_type, movement_zone) = {
        let rider = sim
            .substrate
            .entities
            .get(passenger)
            .ok_or(DepartureFailure::MissingPassenger)?;
        let object_type = sim.object_type(rider.type_ref(), rules);
        (
            rider
                .locomotor
                .as_ref()
                .map(|locomotor| locomotor.speed_type())
                .or_else(|| object_type.map(|object_type| object_type.speed_type))
                .unwrap_or(crate::rules::locomotor_type::SpeedType::Foot),
            rider
                .locomotor
                .as_ref()
                .map(|locomotor| locomotor.movement_zone())
                .or_else(|| object_type.map(|object_type| object_type.movement_zone))
                .unwrap_or(crate::rules::locomotor_type::MovementZone::Normal),
        )
    };
    let path_grid = sim.path_grid_snapshot();
    let path_grid = path_grid.as_deref();
    let land_passable = sim
        .resolved_terrain
        .as_ref()
        .and_then(|terrain| terrain.cell(rx, ry))
        .map(|cell| {
            cell.speed_costs
                .cost_for_speed_type(speed_type)
                .is_none_or(|cost| cost > 0)
        })
        .unwrap_or_else(|| path_grid.is_none_or(|grid| grid.is_walkable(rx, ry)));
    let passability = if path_grid.is_none() && sim.resolved_terrain.is_none() {
        // Headless fixtures have no Cell substrate to ask.
        IsClearToMoveResult::Clear {
            selected_layer: MovementLayer::Ground,
        }
    } else {
        evaluate_live_cell_passability(LiveCellPassabilityQuery {
            target: (rx, ry),
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
        })
    };
    match passability {
        IsClearToMoveResult::Clear {
            selected_layer: MovementLayer::Ground | MovementLayer::Bridge,
        }
        | IsClearToMoveResult::ClearWinged => {}
        _ => return Err(DepartureFailure::Placement),
    }

    // Unlimbo places the passenger at the spot PlaceInfantryInCell chose; the
    // falling Z keeps the plane's. OnBridge, from this cell or an earlier
    // refused one, makes GetHeight measure the fall to the deck's height.
    let on_bridge = sim
        .substrate
        .entities
        .get_mut(passenger)
        .is_some_and(|rider| {
            if rider.category() == EntityCategory::Infantry {
                rider.sub_cell.hold(spot);
            } else {
                rider.sub_cell.release();
            }
            rider.passenger_role.clear_carrier_relation();
            rider.on_bridge()
        });
    if !begin_parachute_descent(&mut sim.substrate.entities, passenger, landing.z) {
        return Err(DepartureFailure::ParachuteAttach);
    }
    let level = sim
        .resolved_terrain
        .as_ref()
        .and_then(|terrain| terrain.cell(rx, ry))
        .map_or(0, |cell| cell.level.wrapping_add(u8::from(on_bridge) * 4));
    let outcome = sim.try_reveal_entity_with_context(
        passenger,
        RevealRequest {
            position: RevealPosition::new(rx, ry, level, None, position.sub_x(), position.sub_y()),
            placement: PlacementEvidence::MarkSucceeded,
            logic_eligible: true,
        },
        UninitContext::new(Some(rules), registry)
            .with_effects(effects)
            .with_unlimbo_facing(Some(PARADROP_UNLIMBO_FACING)),
    );
    if !matches!(outcome, RevealOutcome::Revealed { .. }) {
        return Err(DepartureFailure::ParachuteReveal(outcome));
    }
    sim.attach_parachute_anim(rules, passenger);
    Ok(())
}

/// Drop_Payload's success tail (`0x00415DF6..0x00415EAA`).
fn land_tail(
    sim: &mut Simulation,
    rules: &RuleSet,
    plane: u64,
    passenger: u64,
    landing: DriveCoord,
    effects: FrameEffects<'_>,
) {
    let Some(plane_entity) = sim.substrate.entities.get(plane) else {
        return;
    };
    #[cfg(any(test, feature = "test-support"))]
    {
        let location = ground_pose::object_location(plane_entity, sim.resolved_terrain.as_ref());
        observe(Observed::PlayAt([location.x, location.y, location.z]));
    }
    sim.sound_events.push(SimSoundEvent::ChuteSound {
        rx: plane_entity.position.rx(),
        ry: plane_entity.position.ry(),
    });
    #[cfg(any(test, feature = "test-support"))]
    observe(Observed::CellAt([landing.x, landing.y, landing.z]));
    sim.foot_neighbors_after_payload_drop(
        passenger,
        ((landing.x / 256) as i16, (landing.y / 256) as i16),
    );
    if let Some((team, _)) = sim.team_script_vm.team_for_member(plane) {
        #[cfg(any(test, feature = "test-support"))]
        observe(Observed::RemoveMember(passenger));
        sim.team_remove_member(team, passenger, false, Some(rules), effects);
    }
    let frame = sim.session.binary_frame as i32;
    if let Some(plane_entity) = sim.substrate.entities.get_mut(plane) {
        plane_entity.mission_leaf.restore_paradrop_passes();
        plane_entity.rearm_timer.start(frame, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::playfield::PlayfieldBounds;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::ammo::TechnoAmmo;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::passenger::PassengerCargo;
    use crate::sim::world::RevealOutcome;
    use crate::util::lepton;

    fn drop_test_rules() -> RuleSet {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             0=E1\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             0=PDPLANE\n\
             [BuildingTypes]\n\
             [Warheads]\n\
             0=SHOTWH\n\
             [E1]\n\
             Name=GI\n\
             Strength=100\n\
             Size=1\n\
             [PDPLANE]\n\
             Name=Paradrop Plane\n\
             Strength=400\n\
             Ammo=100\n\
             [SHOTWH]\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             InfDeath=1\n",
        );
        RuleSet::from_ini(&ini).expect("drop test rules should parse")
    }

    /// A playfield holding every cell the drops below use: the coordinate
    /// sum above 10 and at most 132, and `y - x` below 10.
    const DROP_PLAYFIELD: PlayfieldBounds = PlayfieldBounds {
        base: 10,
        off_fc: 0,
        off_100: 0,
        off_104: 60,
        off_108: 60,
    };

    /// Plane 1 over (50, 20) heading south with Ammo 4 (an odd 3 after the
    /// decrement, so the drop turns left) and passenger 2 aboard.
    fn insert_loaded_paradrop_pair(sim: &mut Simulation, aircraft_id: u64, passenger_id: u64) {
        sim.playfield_bounds = Some(DROP_PLAYFIELD);
        let mut aircraft = GameEntity::test_default_of_category(
            aircraft_id,
            "PDPLANE",
            "Americans",
            50,
            20,
            EntityCategory::Aircraft,
        );
        aircraft.set_owner_for_test(sim.interner.intern("Americans"));
        aircraft.set_type_ref_for_test(sim.interner.intern("PDPLANE"));
        aircraft.body_facing.snap(0x8000, 0);
        aircraft.install_ammo_for_test(Some(TechnoAmmo::new(4)));
        let mut cargo = PassengerCargo::new(8, 0);
        cargo.board_forced(passenger_id, 1);
        aircraft.passenger_role = PassengerRole::transport(cargo);
        sim.substrate.entities.insert(aircraft);

        let mut passenger = GameEntity::new_at_frame_zero_for_test(
            passenger_id,
            50,
            20,
            0,
            0,
            sim.interner.intern("Americans"),
            crate::sim::combat::Health::new(100),
            sim.interner.intern("E1"),
            EntityCategory::Infantry,
            0,
            0,
            false,
        );
        passenger.set_is_voxel_for_test(false);
        passenger.set_sub_cell(Some(2));
        passenger.passenger_role = PassengerRole::inside_for_test(aircraft_id, false);
        sim.substrate.entities.insert(passenger);
    }

    fn set_ammo(sim: &mut Simulation, id: u64, ammo: i32) {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .ammo_mut()
            .unwrap()
            .set_current_for_test(ammo);
    }

    fn ammo(sim: &Simulation, id: u64) -> i32 {
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .ammo()
            .unwrap()
            .current()
    }

    fn cargo(sim: &Simulation, id: u64) -> Vec<u64> {
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap()
            .passengers()
            .to_vec()
    }

    fn chute_sounds(sim: &Simulation) -> Vec<(u16, u16)> {
        sim.sound_events
            .iter()
            .filter_map(|event| match event {
                SimSoundEvent::ChuteSound { rx, ry } => Some((*rx, *ry)),
                _ => None,
            })
            .collect()
    }

    fn block_three_spots(sim: &mut Simulation, cell: (u16, u16)) {
        for (id, subcell) in [(90, 2), (91, 3), (92, 4)] {
            let mut blocker = GameEntity::test_default(id, "E1", "Americans", cell.0, cell.1);
            blocker.set_owner_for_test(sim.interner.intern("Americans"));
            blocker.set_type_ref_for_test(sim.interner.intern("E1"));
            blocker.set_category_for_test(EntityCategory::Infantry);
            blocker.set_is_voxel_for_test(false);
            blocker.set_sub_cell(Some(subcell));
            {
                let (sub_x, sub_y) = lepton::subcell_lepton_offset(Some(subcell));
                blocker.position.set_sub_x_for_test(sub_x);
                blocker.position.set_sub_y_for_test(sub_y);
            }
            sim.substrate.entities.insert(blocker);
            assert!(matches!(sim.reveal(id), RevealOutcome::Revealed { .. }));
        }
    }

    #[test]
    fn paradrop_landing_cell_matches_original_coordinate_prefix() {
        let oracle: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/paradrop_coordinates.json",
        ))
        .unwrap();
        let rules = drop_test_rules();
        let mut checked = 0;
        // All selected headings for both parities at (12928,5248). These are
        // original coordinate outputs, before native admission/subcell choice.
        for sweep in &oracle["sweeps"].as_array().unwrap()[..2] {
            assert_eq!(sweep["origin"], serde_json::json!([12928, 5248]));
            for row in sweep["samples"].as_array().unwrap() {
                let mut sim = Simulation::new();
                insert_loaded_paradrop_pair(&mut sim, 1, 2);
                let plane = sim.substrate.entities.get_mut(1).unwrap();
                plane.position.put_world_xy([12928, 5248]);
                plane
                    .body_facing
                    .snap(row["facing"].as_u64().unwrap() as u16, 0);
                set_ammo(
                    &mut sim,
                    1,
                    sweep["post_count"].as_i64().unwrap() as i32 + 1,
                );
                let expected_cell = (
                    lepton::lepton_to_cell(row["world_xy"][0].as_i64().unwrap() as i32) as u16,
                    lepton::lepton_to_cell(row["world_xy"][1].as_i64().unwrap() as i32) as u16,
                );
                drop_payload(
                    &mut sim,
                    1,
                    &rules,
                    None,
                    crate::sim::world::FrameEffects::default(),
                );
                assert!(cargo(&sim, 1).is_empty(), "native row={row}");
                let passenger = sim.substrate.entities.get(2).unwrap();
                assert_eq!(
                    (passenger.position.rx(), passenger.position.ry()),
                    expected_cell,
                    "native row={row}, post_count={}",
                    sweep["post_count"],
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 52);
    }

    #[test]
    fn paradrop_world_coordinates_match_all_native_headings() {
        use crate::util::sha256::{Sha256, digest_hex};
        let oracle: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/paradrop_coordinates.json",
        ))
        .unwrap();
        let sweeps = oracle["sweeps"].as_array().unwrap();
        assert_eq!(sweeps.len(), 6);
        for (index, sweep) in sweeps.iter().enumerate() {
            let origin = [[12928, 5248], [0, 0], [131071, 130816]][index / 2];
            let post_count = (index % 2) as i32;
            assert_eq!(sweep["origin"], serde_json::json!(origin));
            assert_eq!(sweep["post_count"], post_count);
            assert_eq!(sweep["facing_count"], 65536);
            for sample in sweep["samples"].as_array().unwrap() {
                assert_eq!(
                    serde_json::json!(drop_world_xy(
                        origin,
                        sample["facing"].as_u64().unwrap() as u16,
                        post_count
                    )),
                    sample["world_xy"],
                    "origin={origin:?}, post={post_count}, sample={sample}",
                );
            }
            let mut digest = Sha256::new();
            for facing in 0..=u16::MAX {
                for coordinate in drop_world_xy(origin, facing, post_count) {
                    digest.update(&coordinate.to_le_bytes());
                }
            }
            assert_eq!(digest_hex(digest.finalize()), sweep["world_xy_sha256"]);
        }
    }

    #[test]
    fn paradrop_uses_native_cell_for_occupancy_admission() {
        let oracle: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/paradrop_coordinates.json",
        ))
        .unwrap();
        let row = oracle["sweeps"][0]["samples"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["facing"] == 0x7F)
            .unwrap();
        let native_cell = (
            lepton::lepton_to_cell(row["world_xy"][0].as_i64().unwrap() as i32) as u16,
            lepton::lepton_to_cell(row["world_xy"][1].as_i64().unwrap() as i32) as u16,
        );
        let rules = drop_test_rules();
        for blocked in [false, true] {
            let mut sim = Simulation::new();
            insert_loaded_paradrop_pair(&mut sim, 1, 2);
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .body_facing
                .snap(0x7F, 0);
            // An even Ammo after the decrement turns right.
            set_ammo(&mut sim, 1, 1);
            // Opposite admission outcomes on the two adjacent cells: the old
            // byte-facing/offset path chose (51,20), the native row (50,20).
            block_three_spots(&mut sim, if blocked { native_cell } else { (51, 20) });
            drop_payload(
                &mut sim,
                1,
                &rules,
                None,
                crate::sim::world::FrameEffects::default(),
            );
            let passenger = sim.substrate.entities.get(2).unwrap();
            if blocked {
                assert_eq!(cargo(&sim, 1), vec![2]);
                assert!(passenger.lifecycle.in_limbo());
                assert!(!passenger.is_falling_down());
            } else {
                assert!(cargo(&sim, 1).is_empty());
                assert_eq!(
                    (passenger.position.rx(), passenger.position.ry()),
                    native_cell
                );
                assert!(!passenger.lifecycle.in_limbo());
            }
        }
    }

    /// The production path end to end: the drop attaches the canopy, the
    /// object turn winds it down on the landing edge, and the store plays it
    /// out to its last frame and removes it (`ObjectClass::Paradrop
    /// 0x005F5A9D`, `ObjectClass::AI 0x005F3F9D`, `AnimClass::AI 0x0042475A`).
    #[test]
    fn a_drop_attaches_a_canopy_that_winds_down_at_landing_and_plays_out() {
        use crate::rules::art_data::ArtRegistry;
        let mut sim = Simulation::new();
        let mut rules = drop_test_rules();
        rules.general.parachute_shp = Some("PARACH".to_string());
        let mut art = ArtRegistry::from_ini(&IniFile::from_str(
            "[PARACH]\nRate=900\nLoopStart=2\nLoopEnd=5\nLoopCount=-1\n",
        ));
        art.bind_anim_frame_count_for_test("PARACH", 9);
        // Synthetic fixture supplies ART directly, without native read-admission replay.
        rules.install_art_fixture(art);
        // The fixture places ids 1 and 2 by hand; keep the counter clear of them.
        let (aircraft_id, passenger_id) = (sim.allocate_stable_id(), sim.allocate_stable_id());
        insert_loaded_paradrop_pair(&mut sim, aircraft_id, passenger_id);

        drop_payload(
            &mut sim,
            aircraft_id,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert!(cargo(&sim, aircraft_id).is_empty());
        let parachute = sim.interner.get("PARACH").expect("type interned");
        let canopy = |sim: &Simulation| {
            sim.substrate
                .anims
                .iter()
                .find(|(_, anim)| anim.type_id == parachute)
                .map(|(id, anim)| (*id, anim.owner_entity, anim.runtime.loop_remaining))
        };
        let (canopy_id, owner, loops) = canopy(&sim).expect("the drop attached a canopy");
        assert_eq!((owner, loops), (Some(passenger_id), u8::MAX));

        let mut wound_down = false;
        let mut played_out = false;
        for _ in 0..60 {
            sim.advance_tick(&[], Some(&rules), None, None, 100);
            let falling = sim
                .substrate
                .entities
                .get(passenger_id)
                .is_some_and(|passenger| passenger.is_falling_down());
            match canopy(&sim) {
                Some((id, owner, loops)) => {
                    assert_eq!((id, owner), (canopy_id, Some(passenger_id)));
                    assert_eq!(loops == 0, !falling, "zeroed exactly at the landing");
                    wound_down |= loops == 0;
                }
                None => {
                    played_out = true;
                    break;
                }
            }
        }
        assert!(wound_down, "the landing zeroed the remaining loops");
        assert!(played_out, "the canopy left at its last frame");
        assert!(sim.substrate.entities.get(passenger_id).is_some());
    }

    /// A paratrooper shot dead in its fall still holds Paradrop (`+0x6C4`
    /// 0x21), so InfantryClass::ReceiveDamage kills it as InfDeath 3 whatever
    /// the warhead says (`0x0051836F..0x0051842F`): InfantryExplode at its
    /// Location (`0x00518647..0x00518698`), then UnInit (`0x00518B9A`). Its
    /// canopy goes with it (`AnimClass::PointerExpired @ 0x00425150`).
    #[test]
    fn a_paratrooper_shot_in_its_fall_explodes_and_leaves() {
        use crate::rules::art_data::ArtRegistry;
        use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
        let mut sim = Simulation::new();
        let mut rules = drop_test_rules();
        rules.general.parachute_shp = Some("PARACH".to_string());
        // An E1 sequence with Paradrop frames, as retail's GI has: without them
        // Do_Action refuses the drop's Paradrop (`0x0051D70F`).
        let art_ini = IniFile::from_str(
            "[PARACH]\nRate=900\nLoopStart=2\nLoopEnd=5\nLoopCount=-1\n\
             [S_BANG34]\nRate=900\n\
             [E1]\nSequence=E1Sequence\n\
             [E1Sequence]\nReady=0,1,1\nGuard=0,1,1\nWalk=8,6,6\nDie1=134,15,0\n\
             Die2=149,15,0\nParadrop=418,1,0\n",
        );
        let mut art = ArtRegistry::from_ini(&art_ini);
        art.bind_anim_frame_count_for_test("PARACH", 9);
        art.bind_anim_frame_count_for_test("S_BANG34", 9);
        rules.install_art_fixture(art);
        let sequences = crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art_ini);
        rules.replace_animation_sequences_for_test(
            crate::rules::animation_sequence::build_animation_sequence_catalog(
                &rules,
                Some(&sequences),
            ),
        );
        let (aircraft_id, passenger_id) = (sim.allocate_stable_id(), sim.allocate_stable_id());
        insert_loaded_paradrop_pair(&mut sim, aircraft_id, passenger_id);
        sim.substrate
            .entities
            .get_mut(aircraft_id)
            .unwrap()
            .position
            .set_exact_z_leptons_for_test(Some(1500));
        drop_payload(
            &mut sim,
            aircraft_id,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        for _ in 0..10 {
            sim.advance_tick(&[], Some(&rules), None, None, 100);
        }
        let passenger = sim.substrate.entities.get(passenger_id).unwrap();
        assert!(passenger.is_falling_down());
        assert_eq!(
            passenger
                .mission_leaf
                .as_infantry()
                .map(|leaf| leaf.doing()),
            Some(DO_PARADROP)
        );
        let at = crate::sim::movement::ground_pose::position_world_coord(&passenger.position);
        assert!(at.z > 1400, "still high in its fall: {at:?}");

        let warhead = sim.interner.intern("SHOTWH");
        let hit = EntityDamageEvent::direct_receiver(
            passenger_id,
            1000,
            0,
            RAD_NO_ATTACKER,
            None,
            warhead,
            ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        );
        sim.commit_noncombat_aoe_hits(
            &rules,
            None,
            &[hit],
            crate::sim::world::FrameEffects::default(),
        );

        assert!(
            sim.substrate
                .entities
                .get(passenger_id)
                .is_none_or(|passenger| !passenger.lifecycle.object_alive()),
            "UnInit at the hit"
        );
        let explode = sim.interner.get("S_BANG34").expect("InfantryExplode built");
        let explosions: Vec<_> = sim
            .substrate
            .anims
            .iter()
            .filter(|(_, anim)| anim.type_id == explode)
            .map(|(_, anim)| anim.world_coord)
            .collect();
        assert_eq!(explosions.len(), 1, "{explosions:?}");
        assert_eq!(
            (explosions[0].x, explosions[0].y, explosions[0].z),
            (at.x, at.y, at.z),
            "at its Location"
        );
        let parachute = sim.interner.get("PARACH").expect("canopy interned");
        assert!(
            sim.substrate
                .anims
                .iter()
                .filter(|(_, anim)| anim.type_id == parachute)
                .all(|(_, anim)| anim.owner_entity.is_none() && anim.runtime.inactive),
            "the canopy's owner expired"
        );
    }

    /// The drop coordinate is the plane's GetCoords (`0x00415C93`) with only
    /// its XY moved to the landing spot (`0x00415DD7..0x00415DDF`), and
    /// Paradrop places the passenger there (`0x005F5A50`). Dropped from a
    /// plane over higher ground than its landing spot, a paratrooper starts at
    /// the plane's Z, not at the plane's altitude above the landing ground.
    #[test]
    fn a_paratrooper_starts_falling_at_the_planes_z() {
        use crate::map::resolved_terrain::{
            ResolvedTerrainGrid, TEST_OPEN_SPEED_COSTS, test_flat_cell,
        };
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::sim::movement::locomotor::LocomotorState;

        let mut sim = Simulation::new();
        let cells = (0..64u16)
            .flat_map(|y| {
                (0..64u16).map(move |x| {
                    let mut cell = test_flat_cell(x, y);
                    // Open ground: the passenger's Can_Enter_Cell admits it.
                    cell.speed_costs = TEST_OPEN_SPEED_COSTS;
                    cell.base_speed_costs = TEST_OPEN_SPEED_COSTS;
                    if (x, y) == (50, 20) {
                        cell.level = 2;
                    }
                    cell
                })
            })
            .collect();
        sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(64, 64, cells));
        let rules = drop_test_rules();
        let (aircraft_id, passenger_id) = (1, 2);
        insert_loaded_paradrop_pair(&mut sim, aircraft_id, passenger_id);
        // In flight 1500 leptons over its level-2 cell, as the Fly host keeps it.
        let plane_z = 2 * 104 + 1500;
        let plane = sim.substrate.entities.get_mut(aircraft_id).unwrap();
        plane.position.set_exact_z_leptons_for_test(Some(plane_z));
        plane.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Fly));

        drop_payload(
            &mut sim,
            aircraft_id,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        let passenger = sim.substrate.entities.get(passenger_id).unwrap();
        assert_eq!(
            (passenger.position.rx(), passenger.position.ry()),
            (51, 20),
            "the landing cell is at level 0"
        );
        assert!(passenger.is_falling_down());
        assert_eq!(passenger.position.exact_z_leptons(), Some(plane_z));
    }

    /// Over a structural bridge cell (`+0x140 & 0x100`) Paradrop sets OnBridge
    /// (`0x005F5986`) and refuses the cell unless it also has the `0x200` flag
    /// (`0x005F598D..0x005F5996`). The refused passenger boards again with
    /// OnBridge still set.
    #[test]
    fn a_drop_over_a_bridge_cell_needs_its_0x200_flag() {
        use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};

        let rules = drop_test_rules();
        let drop = |flags: u32| {
            let mut sim = Simulation::new();
            let cells = (0..64u16)
                .flat_map(|y| {
                    (0..64u16).map(move |x| {
                        let mut cell = test_flat_cell(x, y);
                        if (x, y) == (51, 20) {
                            cell.bridge_facts.raw_flags = flags;
                            cell.has_bridge_deck = true;
                            cell.bridge_deck_level = 4;
                        }
                        cell
                    })
                })
                .collect();
            sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(
                64, 64, cells,
            ));
            insert_loaded_paradrop_pair(&mut sim, 1, 2);
            drop_payload(
                &mut sim,
                1,
                &rules,
                None,
                crate::sim::world::FrameEffects::default(),
            );
            let passenger = sim.substrate.entities.get(2).unwrap();
            (
                passenger.passenger_role.is_inside_transport(),
                passenger.on_bridge(),
            )
        };

        assert_eq!(drop(BRIDGE_FLAG_STRUCTURAL), (true, true));
        assert_eq!(
            drop(BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_TRANSITION),
            (false, true)
        );
    }

    /// A passenger refused over a bridge cell keeps OnBridge aboard, so its
    /// next drop over plain ground falls to the deck's height: GetHeight
    /// takes the deck off while OnBridge (`0x005F5F40`), and the landing's
    /// SetHeight(0) (`0x005F3F7A`) puts it back (`0x005F5FA0`). It stands on
    /// that ground cell's bridge list, as Mark files it by OnBridge.
    #[test]
    fn a_passenger_refused_over_a_bridge_lands_at_deck_height_on_ground() {
        use crate::map::resolved_terrain::{
            ResolvedTerrainGrid, TEST_OPEN_SPEED_COSTS, test_flat_cell,
        };
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::sim::movement::locomotor::LocomotorState;

        let rules = drop_test_rules();
        let mut sim = Simulation::new();
        let cells = (0..64u16)
            .flat_map(|y| {
                (0..64u16).map(move |x| {
                    let mut cell = test_flat_cell(x, y);
                    cell.speed_costs = TEST_OPEN_SPEED_COSTS;
                    cell.base_speed_costs = TEST_OPEN_SPEED_COSTS;
                    if (x, y) == (51, 20) {
                        cell.bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
                        cell.has_bridge_deck = true;
                        cell.bridge_deck_level = 4;
                    }
                    cell
                })
            })
            .collect();
        sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(64, 64, cells));
        let (plane, passenger) = (1, 2);
        insert_loaded_paradrop_pair(&mut sim, plane, passenger);
        let plane_entity = sim.substrate.entities.get_mut(plane).unwrap();
        plane_entity
            .position
            .set_exact_z_leptons_for_test(Some(1500));
        plane_entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Fly));
        let effects = crate::sim::world::FrameEffects::default();

        drop_payload(&mut sim, plane, &rules, None, effects);
        let rider = sim.substrate.entities.get(passenger).unwrap();
        assert!(rider.passenger_role.is_inside_transport());
        assert!(rider.on_bridge(), "the refusal keeps OnBridge");

        // The plane flies on; its next drop is over plain ground at (51, 24).
        let plane_entity = sim.substrate.entities.get_mut(plane).unwrap();
        plane_entity
            .position
            .put_world_xy([50 * 256 + 128, 24 * 256 + 128]);
        drop_payload(&mut sim, plane, &rules, None, effects);
        let rider = sim.substrate.entities.get(passenger).unwrap();
        assert_eq!((rider.position.rx(), rider.position.ry()), (51, 24));
        assert!(rider.on_bridge());

        let max_fall_rate = rules.general.parachute_max_fall_rate;
        let mut frames = 0;
        while !sim.advance_fall(passenger, max_fall_rate, Some(&rules), None, effects) {
            frames += 1;
            assert!(frames < 10_000, "the fall ends");
        }
        let rider = sim.substrate.entities.get(passenger).unwrap();
        assert!(rider.on_bridge());
        assert_eq!(
            ground_pose::object_world_z_leptons(rider, sim.resolved_terrain.as_ref()),
            lepton::BRIDGE_DECK_HEIGHT_LEPTONS,
            "it stands at the deck's height over level-0 ground"
        );
        let cell = sim.substrate.occupancy.get(51, 24).unwrap();
        assert_eq!(cell.snapshot_layer(MovementLayer::Bridge), vec![passenger]);
        assert!(cell.is_empty_on(MovementLayer::Ground));
    }

    /// A drop: the passenger at a free spot of the landing cell, ChuteSound at
    /// the plane's cell (PlayAt at its Location, `0x00415E21`), five passes
    /// back and the rearm timer restarted with no time (`0x00415E88..
    /// 0x00415EAA`).
    #[test]
    fn a_drop_lands_on_a_spot_and_resets_the_planes_passes_and_rearm() {
        let mut sim = Simulation::new();
        let rules = drop_test_rules();
        let (aircraft_id, passenger_id) = (1, 2);
        insert_loaded_paradrop_pair(&mut sim, aircraft_id, passenger_id);
        sim.session.binary_frame = 77;
        let plane = sim.substrate.entities.get_mut(aircraft_id).unwrap();
        plane.mission_leaf.set_paradrop_passes_for_test(-2);
        plane
            .rearm_timer
            .set_for_test(crate::sim::timer::CdTimer::started(3, 40));

        drop_payload(
            &mut sim,
            aircraft_id,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );

        assert_eq!(chute_sounds(&sim), vec![(50, 20)]);
        let plane = sim.substrate.entities.get(aircraft_id).unwrap();
        assert_eq!(
            plane.mission_leaf.as_aircraft().unwrap().paradrop_passes(),
            5
        );
        assert_eq!(
            plane.rearm_timer.timer(),
            crate::sim::timer::CdTimer::started(77, 0)
        );
        assert_eq!(ammo(&sim, aircraft_id), 3);
        let passenger = sim.substrate.entities.get(passenger_id).unwrap();
        assert!(passenger.lifecycle.object_alive());
        assert!(!passenger.lifecycle.in_limbo());
        assert!(passenger.lifecycle.cell_marked());
        assert!(passenger.in_logic_vector.is_member());
        assert_eq!((passenger.position.rx(), passenger.position.ry()), (51, 20));
        let sub_cell = passenger
            .sub_cell()
            .expect("placed infantry should have a subcell");
        assert!(crate::sim::movement::bump_crush::FUNCTIONAL_SUB_CELLS.contains(&sub_cell));
        assert_eq!(
            (passenger.position.sub_x(), passenger.position.sub_y()),
            lepton::subcell_lepton_offset(Some(sub_cell))
        );
        // Unlimbo faces it south (`0x005F5A35`).
        assert_eq!(
            passenger.body_facing_current(sim.session.binary_frame),
            0x8000
        );
        let occupied_subcells: Vec<(u64, u8)> = sim
            .substrate
            .occupancy
            .get(51, 20)
            .expect("drop cell occupied")
            .infantry(MovementLayer::Ground)
            .collect();
        assert_eq!(occupied_subcells, vec![(passenger_id, sub_cell)]);
    }

    /// A refused drop (`0x00415EB1..0x00415EC7`): the passenger back at the
    /// head, in limbo and unmarked, Ammo restored, no sound, and the plane's
    /// passes and rearm timer untouched. A computer-owned passenger forgets
    /// that the current house discovered it (vt+0x11C); a human's keeps it.
    #[test]
    fn a_refused_drop_restores_the_head_and_ammo_and_clears_discovery() {
        let rules = drop_test_rules();
        for human in [false, true] {
            let mut sim = Simulation::new();
            let (aircraft_id, passenger_id) = (1, 2);
            insert_loaded_paradrop_pair(&mut sim, aircraft_id, passenger_id);
            let americans = sim.interner.intern("Americans");
            sim.houses.insert(
                americans,
                crate::sim::house_state::HouseState::new(americans, 0, None, human, 0, 10),
            );
            sim.substrate
                .entities
                .get_mut(passenger_id)
                .unwrap()
                .discovery
                .set_discovered_by_current_house_for_test(true);
            sim.substrate
                .entities
                .get_mut(aircraft_id)
                .unwrap()
                .mission_leaf
                .set_paradrop_passes_for_test(2);
            block_three_spots(&mut sim, (51, 20));
            let rearm = sim
                .substrate
                .entities
                .get(aircraft_id)
                .unwrap()
                .rearm_timer
                .timer();

            drop_payload(
                &mut sim,
                aircraft_id,
                &rules,
                None,
                crate::sim::world::FrameEffects::default(),
            );

            assert!(chute_sounds(&sim).is_empty());
            assert_eq!(cargo(&sim, aircraft_id), vec![passenger_id]);
            assert_eq!(ammo(&sim, aircraft_id), 4);
            let plane = sim.substrate.entities.get(aircraft_id).unwrap();
            assert_eq!(
                plane.mission_leaf.as_aircraft().unwrap().paradrop_passes(),
                2
            );
            assert_eq!(plane.rearm_timer.timer(), rearm);
            let passenger = sim.substrate.entities.get(passenger_id).unwrap();
            assert_eq!(
                passenger.passenger_role.inside_transport_id(),
                Some(aircraft_id)
            );
            assert!(!passenger.is_falling_down());
            assert!(passenger.lifecycle.object_alive() && passenger.lifecycle.in_limbo());
            assert!(!passenger.lifecycle.cell_marked() && !passenger.in_logic_vector.is_member());
            assert_eq!(passenger.discovery.discovered_by_current_house(), human);
            assert!(
                !sim.substrate
                    .occupancy
                    .contains_entity(51, 20, passenger_id)
            );
        }
    }

    #[test]
    fn attach_failed_retry_clears_peer_radio_contact_to_passenger() {
        let mut sim = Simulation::new();
        let rules = drop_test_rules();
        let aircraft_id = 1;
        let missing_passenger_id = 7;
        let peer_id = 9;
        insert_loaded_paradrop_pair(&mut sim, aircraft_id, 2);
        sim.substrate.entities.remove(2);
        let cargo_hold = sim
            .substrate
            .entities
            .get_mut(aircraft_id)
            .unwrap()
            .passenger_role
            .cargo_mut()
            .unwrap();
        cargo_hold.clear_contents();
        cargo_hold.board_forced(missing_passenger_id, 1);

        let mut peer = GameEntity::test_default(peer_id, "E1", "Americans", 11, 10);
        peer.set_owner_for_test(sim.interner.intern("Americans"));
        peer.set_type_ref_for_test(sim.interner.intern("E1"));
        peer.mark_live_contact_with(missing_passenger_id);
        sim.substrate.entities.insert(peer);

        drop_payload(
            &mut sim,
            aircraft_id,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );

        assert!(chute_sounds(&sim).is_empty());
        assert_eq!(cargo(&sim, aircraft_id), vec![missing_passenger_id]);
        assert!(
            !sim.substrate
                .entities
                .get(peer_id)
                .unwrap()
                .has_live_contact_with(missing_passenger_id),
            "attach-failed retry should clear stale peer radio contacts"
        );
    }

    #[test]
    fn cargo_departure_paradrop_preserves_early_state_and_unwinds_reveal_failure() {
        use crate::sim::movement::locomotor::LocomotorState;
        for post_attach in [false, true] {
            let mut sim = Simulation::new();
            let rules = drop_test_rules();
            insert_loaded_paradrop_pair(&mut sim, 1, 2);
            let mut loco = LocomotorState::from_object_type(rules.object("E1").unwrap(), 0);
            loco.set_layer(MovementLayer::Bridge);
            {
                let passenger = sim.substrate.entities.get_mut(2).unwrap();
                passenger.locomotor = Some(loco);
                passenger.lifecycle.set_object_alive_for_test(post_attach);
                passenger.lifecycle.set_cell_marked(post_attach);
            }
            let mut peer = GameEntity::test_default(9, "E1", "Americans", 30, 30);
            peer.mark_live_contact_with(2);
            sim.substrate.entities.insert(peer);
            {
                let aircraft = sim.substrate.entities.get_mut(1).unwrap();
                aircraft.set_gunner_selection_for_test(99, -1);
                let cargo = aircraft.passenger_role.cargo_mut().unwrap();
                // The real passenger (admitted at size 7) at the head, then
                // a size-3 entry: mixed sizes.
                let real = cargo.passengers()[0];
                cargo.set_passengers_for_test(vec![real, 1234]);
                cargo.set_passenger_sizes_for_test(vec![7, 3]);
                cargo.set_total_size_for_test(10);
            }
            let held = serde_json::to_value(
                sim.substrate
                    .entities
                    .get(1)
                    .unwrap()
                    .passenger_role
                    .cargo(),
            )
            .unwrap();
            let rng_before = sim.scenario_rng.state();
            drop_payload(
                &mut sim,
                1,
                &rules,
                None,
                crate::sim::world::FrameEffects::default(),
            );
            let aircraft = sim.substrate.entities.get(1).unwrap();
            assert_eq!(
                serde_json::to_value(aircraft.passenger_role.cargo()).unwrap(),
                held
            );
            assert_eq!(aircraft.current_weapon_number(), 99);
            assert_eq!(ammo(&sim, 1), 4);
            let passenger = sim.substrate.entities.get(2).unwrap();
            assert_eq!(passenger.passenger_role.inside_transport_id(), Some(1));
            assert!(passenger.lifecycle.in_limbo());
            assert_eq!(passenger.lifecycle.cell_marked(), post_attach);
            assert!(!passenger.in_logic_vector.is_member());
            assert!(!passenger.is_falling_down());
            assert_eq!(
                passenger.locomotor.as_ref().unwrap().layer(),
                MovementLayer::Bridge
            );
            assert_eq!(
                sim.substrate
                    .entities
                    .get(9)
                    .unwrap()
                    .has_live_contact_with(2),
                !post_attach
            );
            // Reboarding runs shared Techno Limbo:6F6C6B releases the
            // passenger's Gattling handle even when already in Limbo. A
            // first Foot Limbo may also detach its positional sound. Neither
            // cleanup request is the success-only415DF6 parachute cue.
            let gattling_owner = crate::sim::combat::gattling::gattling_sound_owner(2);
            assert!(
                sim.sound_events.iter().all(|event| match event {
                    SimSoundEvent::ObjectSoundDetached { owner: 2 } => true,
                    SimSoundEvent::GattlingLoopRelease { owner } => *owner == gattling_owner,
                    _ => false,
                }),
                "post_attach={post_attach}: {:?}",
                sim.sound_events
            );
            assert!(chute_sounds(&sim).is_empty());
            if !post_attach {
                assert_eq!(sim.scenario_rng.state(), rng_before);
            }
            if post_attach {
                sim.substrate
                    .entities
                    .get_mut(2)
                    .unwrap()
                    .lifecycle
                    .set_cell_marked(false);
                drop_payload(
                    &mut sim,
                    1,
                    &rules,
                    None,
                    crate::sim::world::FrameEffects::default(),
                );
                let cargo = sim
                    .substrate
                    .entities
                    .get(1)
                    .unwrap()
                    .passenger_role
                    .cargo()
                    .unwrap();
                assert_eq!(cargo.passengers(), vec![1234]);
                assert_eq!(cargo.passenger_sizes(), vec![3]);
                assert_eq!(cargo.total_size(), 3);
                let passenger = sim.substrate.entities.get(2).unwrap();
                assert!(passenger.is_falling_down());
                assert!(passenger.lifecycle.cell_marked() && passenger.in_logic_vector.is_member());
                assert!(!passenger.passenger_role.is_inside_transport());
                assert_eq!(chute_sounds(&sim).len(), 1);
            }
        }
    }
}
