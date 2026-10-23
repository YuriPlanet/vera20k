//! The Spy Plane's missions: `AircraftClass::Mission_SpyplaneApproach @
//! 0x004155F0` (mission 30, vtable `+0x26C`) and
//! `Mission_SpyplaneOverfly @ 0x004157C0` (mission 31, `+0x270`).
//! `MissionClass::AI @ 0x005B3060` runs the current one when its timer is
//! due (jump table `0x005B34E8`, cases 30 and 31) and restarts the timer
//! with the frames it returns ([`super::dispatch_mission`]).
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
//! mission but 1, 27, 30 and 31 (byte table `0x004151C0`,
//! [`super::ai_head_latch_clear`]), so the latch Approach sets holds through
//! Overfly.

use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::mission::MissionType;
use crate::sim::world::FrameEffects;
use crate::sim::world::edge_cell::Edge;
use crate::sim::world::{SimSoundEvent, Simulation, UninitContext};

/// The distance at which Approach turns to Overfly (`0x00415700 CMP
/// EBX,0x300`), in leptons.
const OVERFLY_DISTANCE: i32 = 0x300;

/// The frames Mission_SpyplaneOverfly returns (`0x004158C8`).
const OVERFLY_FRAMES: i32 = 3;

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
pub(super) fn approach(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    frame_effects: FrameEffects<'_>,
) -> i32 {
    let frames = rules.general.spy_plane_camera_frames;
    let Some(entity) = sim.substrate.entities.get(id) else {
        return frames;
    };
    let target = entity.attack_target.target();
    let distance = super::target_distance(sim, id, target);
    let range = weapon(sim, id, rules).map(|(range, _)| range);
    match target {
        None => {
            sim.assign_aircraft_destination(id, None, rules, frame_effects);
            super::queue_mission(&mut sim.substrate, id, MissionType::Retreat);
        }
        Some(target) if super::nav_com_absent(sim, id) => {
            sim.assign_aircraft_destination(id, Some(target.into()), rules, frame_effects);
        }
        Some(_) => {
            if range.is_some_and(|range| distance <= range) {
                snapshot(sim, id, rules, true, frame_effects);
            }
        }
    }
    if distance <= OVERFLY_DISTANCE {
        super::queue_mission(&mut sim.substrate, id, MissionType::SpyplaneOverfly);
        if let Some(entity) = sim.substrate.entities.get_mut(id) {
            entity.mission_leaf.set_aircraft_action_latch(true);
        }
        head_for_opposite_edge(sim, id, rules, frame_effects);
    }
    frames
}

/// `Mission_SpyplaneOverfly @ 0x004157C0`: within the weapon's `Range=` of
/// the Target (0 with none) a snapshot without the sound
/// (`0x004157E3..0x00415854`); with no NavCom the opposite edge's cell, as
/// in Approach (`0x00415859..0x004158C1`). Returns 3.
pub(super) fn overfly(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    frame_effects: FrameEffects<'_>,
) -> i32 {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return OVERFLY_FRAMES;
    };
    let target = entity.attack_target.target();
    let distance = super::target_distance(sim, id, target);
    if weapon(sim, id, rules).is_some_and(|(range, _)| distance <= range) {
        snapshot(sim, id, rules, false, frame_effects);
    }
    if super::nav_com_absent(sim, id) {
        head_for_opposite_edge(sim, id, rules, frame_effects);
    }
    OVERFLY_FRAMES
}

/// GetWeapon(0) (vt+0x3F8 = `0x0070E140`) at the plane's veterancy: its
/// `Range=` in leptons (`+0xB4`) and `Damage=` (`+0xA4`). Native reads the
/// weapon without a NULL test; no retail Spy Plane lacks one.
fn weapon(sim: &Simulation, id: u64, rules: &RuleSet) -> Option<(i32, i32)> {
    let plane = sim.substrate.entities.get(id)?;
    let object = sim.interner.object_type(plane.type_ref(), rules)?;
    let weapon =
        crate::sim::combat::combat_weapon::primary_for_tier(object, plane.veterancy.rank())
            .and_then(|weapon| rules.weapon(weapon))?;
    Some((weapon.range_leptons, weapon.damage))
}

/// The opposite edge (`HouseClass @ 0x0050DAC0`), a cell on it
/// (`PickCellOnEdge @ 0x004AA440`, criterion 4) and, unless that is the
/// empty cell (`0x00889E68`, both words zero), the destination
/// (`MapClass::operator[] @ 0x005657A0`, vt+0x480(cell, 1)).
fn head_for_opposite_edge(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    frame_effects: FrameEffects<'_>,
) {
    let edge = Edge::opposite_edge(sim.aircraft_house_waypoint_edge(id));
    if let Some((rx, ry)) = sim.aircraft_edge_cell(edge).filter(|&cell| cell != (0, 0)) {
        sim.assign_aircraft_destination(id, Some(NavTargetRef::cell(rx, ry)), rules, frame_effects);
    }
}

/// One camera snapshot (`0x00415666..0x004156FB` in Approach,
/// `0x004157EB..0x00415854` in Overfly): `ReReveal` (vt+0x48C =
/// `0x0070B1D0`) and `UpdateReveal` (vt+0x488 = `0x0070AF50`) with the
/// weapon's `Damage=` as the radius ([`Simulation::refresh_object_sight`],
/// past both functions' gates). The snapshot stands until the plane's next
/// `ReReveal`: its next snapshot, or `FootClass::AI`'s 15-frame refresh.
/// Approach then plays `SpyPlaneCamera=` at the plane (`VocClass::PlayAt @
/// 0x007509E0`).
///
/// Then Map567DA0 (4156D1/415854) stores ground edge frames around the
/// current Location, using the live cached sight radius + 3. It runs even
/// when UpdateReveal refuses the map contribution, before Approach's sound.
fn snapshot(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    sound: bool,
    frame_effects: FrameEffects<'_>,
) {
    let Some((_, damage)) = weapon(sim, id, rules) else {
        return;
    };
    sim.refresh_object_sight(id, rules, damage, frame_effects);
    sim.refresh_object_sight_border(
        id,
        UninitContext::new(rules, None).with_effects(frame_effects),
    );
    if sound
        && let Some(camera) = rules.general.spy_plane_camera.clone()
        && let Some(plane) = sim.substrate.entities.get(id)
    {
        let event = SimSoundEvent::voc_at(camera, &plane.position);
        sim.sound_events.emit(event);
    }
}
