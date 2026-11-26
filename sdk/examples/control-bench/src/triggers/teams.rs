//! TeamTypes and the objects stock actions make: the bench's map
//! (`map.ini`, merged into XMP03T4) adds two TeamTypes for the player's
//! house (`House=Americans`): `BenchTeam`, one Tank Destroyer, and
//! `BenchChronoTeam`, one Sniper. Neither type is in the scene otherwise,
//! so a `Birth` bound to the type is the reinforcement's. A Gap Generator
//! (no one else builds one) is the trigger-made building. Each new object
//! is removed once its `Birth` is read.
//!
//! The reinforcements arrive at the map's start waypoints 0..3 (two are
//! free on this four-start map, the other two have a base next to them).
//! The building is tried at each start waypoint in turn until one takes it.
//!
//! `Team` of a member is marked (`triggers.prop-team`, not in `MARKS`) when
//! the engine reads it.

use super::*;

/// The schedule, in frames after the arena opened.
const LOOKUP: i64 = 600;
const REINFORCE: i64 = 610;
const REINFORCE_AT: i64 = 700;
const CHRONO: i64 = 800;
const BUILDING: i64 = 900;
/// Frames between the building's tries.
const BUILDING_EVERY: i64 = 30;

/// Reinforcement phases (`TEAM_PHASE`): which action the next Tank
/// Destroyer's `Birth` belongs to.
const PHASE_REINFORCE: i64 = 1;
const PHASE_REINFORCE_AT: i64 = 2;

/// The work of frame `t` after the arena opened.
pub(super) fn step(t: i64) {
    if t == LOOKUP {
        lookups();
    }
    if !have(TEAM) || !have(CHRONO_TEAM) {
        return;
    }
    match t {
        REINFORCE => {
            st(TEAM_PHASE, PHASE_REINFORCE);
            if Action::reinforce(kind(TEAM)).is_err() {
                crate::bench_fail("triggers.ev-birth-reinforce", "stock 7 answered an error");
            }
        }
        REINFORCE_AT => {
            st(TEAM_PHASE, PHASE_REINFORCE_AT);
            if Action::reinforce_at(kind(TEAM), Waypoint::number(1)).is_err() {
                crate::bench_fail(
                    "triggers.ev-birth-reinforce-at",
                    "stock 80 answered an error",
                );
            }
        }
        CHRONO => {
            if Action::reinforce_by_chrono(kind(CHRONO_TEAM), Waypoint::number(2)).is_err() {
                crate::bench_fail("triggers.ev-birth-chrono", "stock 107 answered an error");
            }
        }
        _ => {}
    }
    // The building, at start waypoint 0, 1, 2 and 3 until one takes it.
    let tries = t - BUILDING;
    if have(TYPE_GAP)
        && (0..4 * BUILDING_EVERY).contains(&tries)
        && tries % BUILDING_EVERY == 0
        && !crate::marked("triggers.ev-birth-building")
    {
        let at = Waypoint::number((tries / BUILDING_EVERY) as u32);
        let _ = Action::create_building_at(at, kind(TYPE_GAP), crate::player());
    }
}

/// The TeamTypes by name, with their names read back, and the `Birth`
/// triggers on the members' and the building's types.
fn lookups() {
    let mut buf = [0u8; 32];
    for (slot, name) in [(TEAM, "BenchTeam"), (CHRONO_TEAM, "BenchChronoTeam")] {
        let Ok(id) = lookup(NameKind::TeamType, name) else {
            crate::bench_fail("triggers.team-lookup", "the map has no bench TeamType");
            return;
        };
        let back = vera20k_sdk::name_of(NameKind::TeamType, id, &mut buf);
        if !back.is_ok_and(|text| text.eq_ignore_ascii_case(name)) {
            crate::bench_fail("triggers.team-lookup", "name_of names another TeamType");
            return;
        }
        // A type reference (never 0), which the stock actions take.
        st(slot, TypeRef::new(NameKind::TeamType, id).0);
    }
    crate::mark("triggers.team-lookup");
    for (slot, name, key) in [
        (TYPE_TNKD, "TNKD", K_BIRTH_TEAM),
        (TYPE_SNIPE, "SNIPE", K_BIRTH_CHRONO),
        (TYPE_GAP, "GAGAP", K_BIRTH_BUILDING),
    ] {
        let Ok(ty) = TypeRef::techno_type(name) else {
            crate::bench_fail("triggers.setup", "a retail TechnoType is missing");
            continue;
        };
        let made = Trigger::on(Event::BIRTH).bind(ty).enqueue(key).done();
        if made.is_ok() {
            st(slot, ty.0);
        } else {
            crate::bench_fail("triggers.create", "a Birth trigger could not be made");
        }
    }
}

/// A `Birth` of this file's triggers: the player's new object.
pub(super) fn record(r: &EventRecord) {
    let made = Techno(r.subject);
    if get(made, Prop::Owner) != Ok(crate::player().0) {
        crate::bench_fail(
            "triggers.ev-birth-team",
            "the new object is not the player's",
        );
        return;
    }
    let name = match r.key {
        K_BIRTH_TEAM if ld(TEAM_PHASE) == PHASE_REINFORCE => "triggers.ev-birth-reinforce",
        K_BIRTH_TEAM if ld(TEAM_PHASE) == PHASE_REINFORCE_AT => "triggers.ev-birth-reinforce-at",
        K_BIRTH_CHRONO => "triggers.ev-birth-chrono",
        K_BIRTH_BUILDING => "triggers.ev-birth-building",
        _ => return,
    };
    crate::mark(name);
    // A member's team, where the engine reads it.
    if r.key != K_BIRTH_BUILDING
        && get(made, Prop::Team).is_ok_and(|team| team != 0 && team != NO_HANDLE)
    {
        crate::mark("triggers.prop-team");
    }
    let _ = Action::remove(made);
}
