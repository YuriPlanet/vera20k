//! The Spy Plane's missions: `AircraftClass::Mission_SpyplaneApproach @
//! 0x004155F0` (mission 30, vtable `+0x26C`) and
//! `Mission_SpyplaneOverfly @ 0x004157C0` (mission 31, `+0x270`).
//! `MissionClass::AI @ 0x005B3060` runs the current one when its timer is
//! due (jump table `0x005B34E8`, cases 30 and 31) and restarts the timer
//! with the frames it returns ([`super::dispatch_native_mission`]).
//!
//! Approach flies at its Target, the clicked cell. Each visit within its
//! weapon's `Range=` of the cell takes a camera snapshot and plays
//! `SpyPlaneCamera=`; at three cells it queues Overfly, latches `+0x6D2`,
//! which freezes the Fly heading so the plane flies on straight, and aims
//! it at the edge across from its house's. Overfly keeps snapshotting until
//! it is out of `Range=`; `aircraft::leave_map` removes the plane past the
//! map's edge.
//!
//! Evidence: `tools/superweapon_oracle.py` section `spyplane_missions` runs
//! both handlers; `spy_plane_tests.rs` replays it.
//!
//! The handlers draw nothing themselves: the Scenario draws are the edge
//! pick's (`PickCellOnEdge @ 0x004AA440`). They write the mission timer and
//! detach nothing. `AircraftClass::AI`'s head clears `+0x6D2` in every
//! mission but 1, 27, 30 and 31 (byte table `0x004151C0`), so the latch
//! Approach sets holds through Overfly; VERA does not port that clear, which
//! this plane never reaches.

use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::edge_cell::{Edge, find_paradrop_edge_cell};
use crate::sim::world::{SimSoundEvent, Simulation};

/// The distance at which Approach turns to Overfly (`0x00415700 CMP
/// EBX,0x300`), in leptons.
const OVERFLY_DISTANCE: i32 = 0x300;

/// The frames Mission_SpyplaneOverfly returns (`0x004158C8`).
const OVERFLY_FRAMES: i32 = 3;

/// Whether `entity` flies a Spy Plane mission, whose handlers below steer
/// it: VERA's pre-combat pursuit stage, a ground stand-in that would halt
/// the plane within its camera's `Range=` of its Target, leaves it alone.
pub(crate) fn steers(entity: &GameEntity) -> bool {
    let current = entity.mission.current();
    current == MissionId::from_known(MissionType::SpyplaneApproach)
        || current == MissionId::from_known(MissionType::SpyplaneOverfly)
}

/// `Mission_SpyplaneApproach @ 0x004155F0`. The distance to the Target
/// (`ObjectClass::Distance_To @ 0x005F6440`, 0 with no Target) comes first.
/// - No Target: no destination (vt+0x480(NULL, 1), `0x00415617`) and
///   Retreat queued (`0x00415625`); the distance of 0 then queues Overfly
///   over it below.
/// - No NavCom (`+0x5A4`): the Target becomes the destination
///   (`0x00415641`), with no snapshot this visit.
/// - Otherwise, within the weapon's `Range=` (GetWeapon(0) `+0xB4`,
///   `0x0041565A`): a snapshot with its sound.
///
/// Then, at 0x300 leptons or less: Overfly queued (`0x00415714`), `+0x6D2`
/// set (`0x00415720`) and the opposite edge's cell, unless it is the empty
/// cell, made the destination (`0x00415727..0x0041577F`). Every path
/// returns `SpyPlaneCameraFrames=` (`Rules+0x290`).
pub(super) fn approach(sim: &mut Simulation, id: u64, rules: &RuleSet) -> i32 {
    let frames = rules.general.spy_plane_camera_frames;
    let Some(entity) = sim.substrate.entities.get(id) else {
        return frames;
    };
    let target = entity.attack_target.as_ref().map(|attack| attack.target);
    let distance = target_distance(sim, id, target);
    let range = weapon(sim, id, rules).map(|(range, _)| range);
    match target {
        None => {
            sim.assign_aircraft_attack_destination(id, None, rules);
            queue(sim, id, MissionType::Retreat);
        }
        Some(target) if nav_com_absent(sim, id) => {
            sim.assign_aircraft_attack_destination(id, Some(target.into()), rules);
        }
        Some(_) => {
            if range.is_some_and(|range| distance <= range) {
                snapshot(sim, id, rules, true);
            }
        }
    }
    if distance <= OVERFLY_DISTANCE {
        queue(sim, id, MissionType::SpyplaneOverfly);
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.mission_leaf.set_aircraft_action_latch(true);
        }
        head_for_opposite_edge(sim, id, rules);
    }
    frames
}

/// `Mission_SpyplaneOverfly @ 0x004157C0`: within the weapon's `Range=` of
/// the Target (0 with none) a snapshot without the sound
/// (`0x004157E3..0x00415854`); with no NavCom the opposite edge's cell, as
/// in Approach (`0x00415859..0x004158C1`). Returns 3.
pub(super) fn overfly(sim: &mut Simulation, id: u64, rules: &RuleSet) -> i32 {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return OVERFLY_FRAMES;
    };
    let target = entity.attack_target.as_ref().map(|attack| attack.target);
    let distance = target_distance(sim, id, target);
    if weapon(sim, id, rules).is_some_and(|(range, _)| distance <= range) {
        snapshot(sim, id, rules, false);
    }
    if nav_com_absent(sim, id) {
        head_for_opposite_edge(sim, id, rules);
    }
    OVERFLY_FRAMES
}

/// `ObjectClass::Distance_To @ 0x005F6440` from the plane; a NULL target
/// answers 0 (`0x005F644B..0x005F6456`).
fn target_distance(sim: &Simulation, id: u64, target: Option<TargetKind>) -> i32 {
    let entities = &sim.substrate.entities;
    target
        .zip(entities.get(id))
        .and_then(|(target, plane)| {
            crate::sim::combat::object_distance_to(plane, &target, entities)
        })
        .unwrap_or(0)
}

/// GetWeapon(0) (vt+0x3F8 = `0x0070E140`) at the plane's veterancy: its
/// `Range=` in leptons (`+0xB4`) and `Damage=` (`+0xA4`). Native reads the
/// weapon without a NULL test; no retail Spy Plane lacks one.
fn weapon(sim: &Simulation, id: u64, rules: &RuleSet) -> Option<(i32, i32)> {
    let plane = sim.substrate.entities.get(id)?;
    let object = rules.object(sim.interner.resolve(plane.type_ref()))?;
    let weapon = crate::sim::combat::combat_weapon::primary_for_tier(object, plane.veterancy())
        .and_then(|weapon| rules.weapon(weapon))?;
    Some((weapon.range_leptons, weapon.damage))
}

fn nav_com_absent(sim: &Simulation, id: u64) -> bool {
    sim.substrate
        .entities
        .get(id)
        .is_some_and(|plane| plane.navigation.nav_com.is_none())
}

/// `Queue_Mission(mission, 0)` (vt+0x1E8 = `0x0041BA90`).
fn queue(sim: &mut Simulation, id: u64, mission: MissionType) {
    if let Some(plane) = sim.substrate.entities.get_mut(id) {
        crate::sim::mission::authority::queue_entity_mission_deferred(
            plane,
            MissionId::from_known(mission),
        );
    }
}

/// The opposite edge (`HouseClass @ 0x0050DAC0`), a cell on it
/// (`PickCellOnEdge @ 0x004AA440`, criterion 4) and, unless that is the
/// empty cell (`0x00889E68`, both words zero), the destination
/// (`MapClass::operator[] @ 0x005657A0`, vt+0x480(cell, 1)).
fn head_for_opposite_edge(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    let owner = sim.substrate.entities.get(id).map(|plane| plane.owner());
    let edge = Edge::opposite_edge(
        owner
            .and_then(|owner| sim.houses.get(&owner))
            .map_or(0, |house| house.waypoint_edge),
    );
    let cell = find_paradrop_edge_cell(
        sim.playfield_bounds,
        sim.resolved_terrain.as_ref(),
        edge,
        &mut sim.scenario_rng,
    );
    if let Some((rx, ry)) = cell.filter(|&cell| cell != (0, 0)) {
        sim.assign_aircraft_attack_destination(id, Some(NavTargetRef::cell(rx, ry)), rules);
    }
}

/// One camera snapshot (`0x00415666..0x004156FB` in Approach,
/// `0x004157EB..0x00415854` in Overfly): `ReReveal` (vt+0x48C =
/// `0x0070B1D0`) and `UpdateReveal` (vt+0x488 = `0x0070AF50`) with the
/// weapon's `Damage=` as the radius, through the vision owner; both skip a
/// plane outside the playfield (`+0x3D5`) or of a MultiplayPassive house
/// (HouseType `+0x1A6`). Approach then plays `SpyPlaneCamera=` at the
/// plane (`VocClass::PlayAt @ 0x007509E0`).
///
/// `MapClass @ 0x00567DA0` (the fog border around the snapshot, radius
/// `LastSightRange + 3`) is presentation: the shroud renderer rebuilds from
/// the vision owner.
///
/// RESIDUAL: VERA rebuilds each object's ordinary sight every frame
/// (`world::refresh_fog`), so the snapshot's admission is released in the
/// frame it is taken, where native keeps it until the plane's next
/// `ReReveal` (its next snapshot, or `FootClass::AI`'s 15-frame refresh).
/// Trigger: a snapshot in a `FogOfWar=` game. Effect: the area fogs over up
/// to 15 frames sooner; its shroud stays lifted either way.
fn snapshot(sim: &mut Simulation, id: u64, rules: &RuleSet, sound: bool) {
    let Some((_, damage)) = weapon(sim, id, rules) else {
        return;
    };
    let Some(plane) = sim.substrate.entities.get(id) else {
        return;
    };
    let passive = sim
        .houses
        .get(&plane.owner())
        .is_some_and(|house| house.multiplay_passive);
    if plane.in_playfield && !passive {
        let config = sim.sight_reveal_config(Some(rules));
        let heights = config
            .reveal_by_height
            .then(|| sim.path_grid().map(|grid| grid.ground_height_grid()))
            .flatten();
        let ability =
            crate::sim::vision::entity_has_sight_ability(plane, &sim.interner, Some(rules));
        crate::sim::vision::force_refresh_entity_vision_at_radius(
            &mut sim.fog,
            plane,
            &config,
            heights.as_deref(),
            ability,
            &sim.interner,
            damage,
        );
    }
    if sound && let Some(camera) = rules.general.spy_plane_camera.clone() {
        let event = SimSoundEvent::voc_at(camera, &plane.position);
        sim.sound_events.push(event);
    }
}
