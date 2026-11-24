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

/// The return the Overfly visit at `0x00415A00` gives after its
/// `Drop_Payload`: Phobos replaces the immediate `5` with the plane's
/// `ParadropDelay=` (passengers left) or `ParadropEndDelay=` (none left),
/// each falling back to its `[General]` value (`Ext/Aircraft/Hooks.cpp:1265-1286`).
/// A negative end delay means the plane never leaves, so the hook returns
/// `INT32_MAX` (`:1280-1281`); the passenger-held delay is passed through
/// unchanged.
fn overfly_delay(
    rules: &RuleSet,
    plane: &crate::rules::object_type::ObjectType,
    carrying: bool,
) -> i32 {
    if !rules.phobos() {
        return if carrying { 5 } else { OVERFLY_FRAMES };
    }
    if carrying {
        return plane
            .phobos_paradrop_delay
            .unwrap_or(rules.phobos_globals.paradrop_delay);
    }
    {
        return end_delay(
            plane.phobos_paradrop_end_delay(),
            rules.phobos_globals.paradrop_end_delay,
        );
    }
}

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
    frame_effects: &mut FrameEffects<'_>,
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
    frame_effects: &mut FrameEffects<'_>,
) -> i32 {
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return OVERFLY_FRAMES;
    };
    entity.mission_leaf.set_aircraft_action_latch(true);
    let target = entity.attack_target.target();
    let carrying = entity
        .passenger_role()
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
        super::drop_payload::drop_payload(sim, id, rules, frame_effects);
        // `0x00415A00` returns the type's carrying/end delay instead of the
        // stock frames (`Ext/Aircraft/Hooks.cpp:1265-1286`). The passenger
        // count is read after the drop, as the hook does.
        let plane = sim
            .substrate
            .entities
            .get(id)
            .and_then(|entity| sim.interner.object_type(entity.type_ref(), rules));
        let carrying = sim
            .substrate
            .entities
            .get(id)
            .and_then(|entity| entity.passenger_role.cargo())
            .is_some_and(|cargo| cargo.count() > 0);
        if let Some(plane) = plane {
            return overfly_delay(rules, plane, carrying);
        }
    }
    OVERFLY_FRAMES
}

/// Phobos `ParadropEndDelay.Get(RulesExt::Global()->ParadropEndDelay)` with
/// the negative special case (`Ext/Aircraft/Hooks.cpp:1278-1282`).
pub(crate) fn paradrop_end_delay(sim: &Simulation, id: u64, rules: &RuleSet) -> i32 {
    if !rules.phobos() {
        return OVERFLY_FRAMES;
    }
    let typed = sim
        .substrate
        .entities
        .get(id)
        .and_then(|plane| sim.object_type(plane.type_ref(), rules))
        .and_then(|object| object.phobos_paradrop_end_delay());
    end_delay(typed, rules.phobos_globals.paradrop_end_delay)
}

/// `ParadropEndDelay`'s negative special case, shared by the overfly return
/// and the test-facing reader.
fn end_delay(typed: Option<i32>, global: i32) -> i32 {
    let delay = typed.unwrap_or(global);
    if delay < 0 { i32::MAX } else { delay }
}

/// Overfly's way out (`0x00415A0A..0x00415A33`): the latch cleared, no
/// Target (vt+0x3C8(NULL)), no destination (vt+0x480(NULL, 1)) and Retreat
/// queued.
fn give_up(sim: &mut Simulation, id: u64, rules: &RuleSet, frame_effects: &mut FrameEffects<'_>) {
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.mission_leaf.set_aircraft_action_latch(false);
        crate::sim::mission::concrete_effects::represented_assign_target(
            entity,
            None,
            rules.ares_form(),
        );
    }
    sim.assign_aircraft_destination(id, None, rules, frame_effects);
    super::queue_mission(&mut sim.substrate, id, MissionType::Retreat);
}

#[cfg(all(test, tests_sim_a_b))]
mod overfly_delay_tests {
    use super::overfly_delay;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;

    fn rules_with(type_body: &str) -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(&format!(
            "[General]\nParadropEndDelay=9\n\
             [AircraftTypes]\n0=PLANE\n\
             [PLANE]\nStrength=100\n{type_body}"
        )))
        .unwrap()
    }

    /// `Ext/Aircraft/Hooks.cpp:1265-1286`: the type keys beat their
    /// `[General]` fallbacks, and only the passenger-free arm maps a negative
    /// value to `INT32_MAX`.
    #[test]
    fn the_overfly_return_uses_the_carrying_and_end_delays() {
        let typed = rules_with("ParadropDelay=8\nParadropEndDelay=12\n");
        let plane = typed.object("PLANE").unwrap();
        assert_eq!(overfly_delay(&typed, plane, true), 8);
        assert_eq!(overfly_delay(&typed, plane, false), 12);

        let unset = rules_with("");
        let plane = unset.object("PLANE").unwrap();
        assert_eq!(
            overfly_delay(&unset, plane, true),
            5,
            "ParadropDelay ctor 5"
        );
        assert_eq!(
            overfly_delay(&unset, plane, false),
            9,
            "[General] ParadropEndDelay"
        );

        let negative = rules_with("ParadropEndDelay=-3\n");
        let plane = negative.object("PLANE").unwrap();
        assert_eq!(overfly_delay(&negative, plane, false), i32::MAX);
        assert_eq!(overfly_delay(&negative, plane, true), 5);
    }
}
