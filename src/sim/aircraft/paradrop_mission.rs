//! The paradrop plane's missions: `AircraftClass::Mission_ParadropApproach @
//! 0x004158E0` (mission 26) and `Mission_ParadropOverfly @ 0x00415960` (27).
//! `MissionClass::AI @ 0x005B3060` runs the current one when its timer is
//! due (jump table `0x005B34E8`, cases 26 and 27) and restarts the timer
//! with the frames it returns ([`super::dispatch_mission`]).
//!
//! Approach flies at its Target, the clicked cell, and turns into Overfly
//! within `[General] ParadropRadius=` (`Rules+0x54C`) of it, spending one of
//! the plane's passes (`+0x6D3`). Overfly latches `+0x6D2`, which freezes
//! the Fly heading so the plane flies on straight, and drops one passenger
//! a visit ([`super::drop_payload`]) while it is within the radius and over
//! the playfield. Past the radius it comes round again while passes remain;
//! with none left, or its passengers gone, it drops its Target and
//! destination and retreats ([`super::retreat_mission`]), and
//! [`super::leave_map`] removes it past the map's edge.
//!
//! Evidence: `tools/superweapon_oracle.py` section `paradrop_missions` runs
//! both handlers; `superweapon/paradrop_tests.rs` replays it.
//!
//! The handlers draw nothing and detach nothing. They write the mission
//! timer, `+0x6D2` and `+0x6D3`; the drop writes the rest.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, sim/aircraft, sim/cell_rect,
//!   sim/mission, sim/world.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::rules::ruleset::RuleSet;
use crate::sim::mission::MissionType;
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;

/// Every Approach visit's return (`0x00415907`, `0x00415925`, `0x00415956`).
const APPROACH_FRAMES: i32 = 3;

/// Every Overfly visit's return (`0x004159BE`, `0x00415A00`, `0x00415A39`).
const OVERFLY_FRAMES: i32 = 5;

/// `Mission_ParadropApproach @ 0x004158E0`:
/// - No Target (`+0x2B4`): no destination (vt+0x480(NULL, 1)) and Retreat
///   queued (`0x004158ED..0x00415901`).
/// - No NavCom (`+0x5A4`): the Target becomes the destination
///   (`0x00415918..0x0041591F`).
/// - Otherwise, within `ParadropRadius=` of the Target
///   (`ObjectClass::Distance_To @ 0x005F6440`, a signed `JG` at
///   `0x00415940`): Overfly queued and one pass taken (`0x00415942..
///   0x00415950`).
pub(super) fn approach(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    frame_effects: FrameEffects<'_>,
) -> i32 {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return APPROACH_FRAMES;
    };
    match entity.attack_target.target() {
        None => {
            sim.assign_aircraft_destination(id, None, rules, frame_effects);
            super::queue_mission(&mut sim.substrate, id, MissionType::Retreat);
        }
        Some(target) if super::nav_com_absent(sim, id) => {
            sim.assign_aircraft_destination(id, Some(target.into()), rules, frame_effects);
        }
        Some(target) => {
            if super::target_distance(sim, id, Some(target)) <= rules.general.paradrop_radius {
                super::queue_mission(&mut sim.substrate, id, MissionType::ParadropOverfly);
                if let Some(entity) = sim.substrate.entities.get_mut(id) {
                    entity.mission_leaf.take_paradrop_pass();
                }
            }
        }
    }
    APPROACH_FRAMES
}

/// `Mission_ParadropOverfly @ 0x00415960`. It latches `+0x6D2` first
/// (`0x0041596C`).
/// - No Target, or no first passenger (`+0x118`): [`give_up`].
/// - Past `ParadropRadius=` of the Target: the latch cleared
///   (`0x004159A5`), then Approach queued again while a pass remains (the
///   signed byte above 0, `0x004159AC`), else [`give_up`].
/// - Within it: one Drop_Payload (`0x004159FB`) while the plane's Location
///   (`+0x9C`) is over the playfield (`MapClass::IsCoordInPlayfield @
///   0x005785F0`).
pub(super) fn overfly(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) -> i32 {
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return OVERFLY_FRAMES;
    };
    entity.mission_leaf.set_aircraft_action_latch(true);
    let target = entity.attack_target.target();
    let carrying = entity
        .passenger_role
        .cargo()
        .is_some_and(|cargo| cargo.count() > 0);
    let Some(target) = target.filter(|_| carrying) else {
        give_up(sim, id, rules, frame_effects);
        return OVERFLY_FRAMES;
    };
    if super::target_distance(sim, id, Some(target)) > rules.general.paradrop_radius {
        let Some(entity) = sim.substrate.entities.get_mut(id) else {
            return OVERFLY_FRAMES;
        };
        entity.mission_leaf.set_aircraft_action_latch(false);
        let passes_left = entity
            .mission_leaf
            .as_aircraft()
            .is_some_and(|leaf| leaf.paradrop_passes() > 0);
        if passes_left {
            super::queue_mission(&mut sim.substrate, id, MissionType::ParadropApproach);
        } else {
            give_up(sim, id, rules, frame_effects);
        }
        return OVERFLY_FRAMES;
    }
    let over_playfield = sim.substrate.entities.get(id).is_some_and(|plane| {
        let location = crate::sim::movement::ground_pose::object_location(
            plane,
            sim.resolved_terrain.as_ref(),
        );
        crate::sim::cell_rect::cell_is_in_playfield_leptons(
            (location.x, location.y, location.z),
            sim.playfield_bounds(),
            sim.resolved_terrain.as_ref(),
        )
    });
    if over_playfield {
        super::drop_payload::drop_payload(sim, id, rules, registry, frame_effects);
    }
    OVERFLY_FRAMES
}

/// Overfly's way out (`0x00415A0A..0x00415A33`): the latch cleared, no
/// Target (vt+0x3C8(NULL)), no destination (vt+0x480(NULL, 1)) and Retreat
/// queued.
fn give_up(sim: &mut Simulation, id: u64, rules: &RuleSet, frame_effects: FrameEffects<'_>) {
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.mission_leaf.set_aircraft_action_latch(false);
        crate::sim::mission::concrete_effects::represented_assign_target(entity, None);
    }
    sim.assign_aircraft_destination(id, None, rules, frame_effects);
    super::queue_mission(&mut sim.substrate, id, MissionType::Retreat);
}
