//! The Spy Plane: `SuperClass::Launch @ 0x006CC390` case 8
//! (`0x006CD66F..0x006CD70B`) and `HouseClass::SendSpyPlanes @ 0x0065EAB0`,
//! which builds each plane and starts it in Mission_SpyplaneApproach. The
//! flight, the camera and the exit are `aircraft::spyplane_mission`; the
//! removal past the map's edge is `aircraft::leave_map`.
//!
//! Evidence: `tools/superweapon_oracle.py` sections `spy_plane_launch` and
//! `send_spy_planes` execute the original code; `spy_plane_tests.rs` replays
//! them.
//!
//! RESIDUALS:
//! - The plane count. Case 8 compares the raw lengths of `AllyParaDropInf=`
//!   and `AllyParaDropNum=` (`Rules+0xC4C`, `+0xC68`) and sends one plane per
//!   type entry. VERA's rules keep only the zipped list
//!   (`ruleset::parse_paradrop_list`), which takes its default on a length
//!   mismatch and holds `E1`/6 when neither key is authored. Trigger: rules
//!   with mismatched or absent lists (retail authors one entry in each).
//!   Effect: one plane where native sends none.
//! - The house's `Edge=` (`HouseClass+0x1E0`), which SendSpyPlanes takes
//!   over the waypoint edge when it is 0..3 (`0x0065EB17..0x0065EB28`). Only
//!   a campaign map's house section sets it; skirmish keeps the
//!   constructor's -1, and VERA reads no house sections. Trigger: a campaign
//!   house with `Edge=`. Effect: its planes enter from the waypoint edge.

use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::edge_cell::{Edge, find_paradrop_edge_cell};
use crate::sim::world::{PlacementEvidence, SimSoundEvent, Simulation};

/// The plane. gamemd holds the literal (`0x00842560`, looked up at
/// `0x006CD67F` through `AircraftTypeClass::FindIndex @ 0x0041CAA0`); no INI
/// key names it.
const SPY_PLANE: &str = "SPYP";

/// Launch case 8 for `owner`'s Spy Plane at `cell`: one plane per
/// `AllyParaDropInf=` entry, each targeting the cell. Returns whether a plane
/// took off.
///
/// The case runs only for a charged Super (`+0x6F`, `0x006CD66F`), which
/// ClickFire already admitted. It finds the plane's type (`0x0041CAA0`, -1
/// when no `[AircraftTypes]` entry is named `SPYP`) and the cell
/// (`MapClass::operator[] @ 0x005657A0`); the shared dummy cell, which an
/// off-map cell answers, sends nothing (`0x006CD69A..0x006CD6A4`), and
/// neither does an unknown type, which the loop tests per plane
/// (`0x006CD6C2`). The local player's launch then drops a queued
/// `EVA_SpyPlaneReady` (`0x006CD6E9..0x006CD707`, the app's
/// `launch_drops_ready_line`), whatever was sent.
pub(super) fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type: InternedId,
    (rx, ry): (u16, u16),
) -> bool {
    sim.sound_events.push(SimSoundEvent::SuperWeaponLaunched {
        owner,
        sw_type,
        rx,
        ry,
    });
    let known_type = rules
        .aircraft_ids
        .iter()
        .any(|id| id.eq_ignore_ascii_case(SPY_PLANE));
    let real_cell = sim.resolved_terrain.as_ref().is_some_and(|terrain| {
        matches!(
            NativeCellQuery::canonical(terrain).lookup((rx as i16, ry as i16)),
            NativeCellIdentity::Real(_)
        )
    });
    if !real_cell || !known_type {
        return false;
    }
    let mut sent = false;
    for _ in 0..rules.general.ally_paradrop_list.len() {
        sent |= send_spy_plane(sim, rules, owner, (rx, ry));
    }
    sent
}

/// `HouseClass::SendSpyPlanes @ 0x0065EAB0` as case 8 calls it: one plane,
/// Mission_SpyplaneApproach, the clicked cell as its target and no
/// destination. Returns whether the plane took off.
///
/// The plane is constructed (`CreateObject`, vt+0x8C) inside the
/// ScenarioInit bracket (`0x00A8E7AC`, `0x0065EAE3..0x0065EB04`) and marked
/// mission-only (`+0x3D4`, `0x0065EB10`) before the edge pick, so its
/// constructor's draws come before the pick's, as in SendParadropPlanes. The
/// pick is `PickCellOnEdge` (`0x004AA440`) on the house's edge; the plane
/// queues the mission (vt+0x1E8, `0x0065EB5E`), takes the cell as its Target
/// (vt+0x3C8, `0x0065EB82`) and Unlimbos at the picked cell's centre, height
/// 0 and facing 0, inside the bracket again (vt+0xD8, `0x0065EB88..
/// 0x0065EBDD`); Aircraft Unlimbo raises a mission-only plane to its
/// FlightLevel. A refused Unlimbo deletes the plane (vt+0x20,
/// `0x0065EC12..0x0065EC1C`); otherwise the queued mission starts at once
/// (vt+0x1EC = `0x0041B870`, `0x0065EBE9`).
///
/// The plane runs no VERA-only aircraft mission: its native mission
/// (`aircraft::spyplane_mission`) is the whole of its behavior.
fn send_spy_plane(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    target: (u16, u16),
) -> bool {
    let owner_name = sim.interner.resolve(owner).to_string();
    let plane = sim.with_object_placement_scope(|sim| {
        sim.construct_object_limbo_at_height(SPY_PLANE, &owner_name, 0, 0, 0, 0, rules)
    });
    let Some(plane) = plane else {
        return false;
    };
    let entity = sim
        .substrate
        .entities
        .get_mut(plane)
        .expect("constructed spy plane");
    entity.mark_mission_only();
    entity.aircraft_mission = None;
    let edge = Edge::own_edge(
        sim.houses
            .get(&owner)
            .map_or(0, |house| house.waypoint_edge),
    );
    let Some(cell) = find_paradrop_edge_cell(
        sim.playfield_bounds,
        sim.resolved_terrain.as_ref(),
        edge,
        &mut sim.scenario_rng,
    ) else {
        // No MapClass authority (headless fixtures): no edge to pick.
        let _ = sim.discard_constructed_limbo(plane, Some(rules));
        return false;
    };
    let entity = sim
        .substrate
        .entities
        .get_mut(plane)
        .expect("constructed spy plane");
    // Case 8's mission argument (`0x006CD6CD PUSH 0x1E`).
    crate::sim::mission::authority::queue_entity_mission_deferred(
        entity,
        MissionId::from_known(MissionType::SpyplaneApproach),
    );
    crate::sim::mission::concrete_effects::represented_assign_target(
        entity,
        Some(TargetKind::Cell(target.0, target.1)),
    );
    let placed = sim.with_object_placement_scope(|sim| {
        sim.reveal_constructed_object_at_height(
            plane,
            cell.0,
            cell.1,
            0,
            0,
            PlacementEvidence::MarkSucceeded,
            rules,
        )
    });
    if placed.is_none() {
        let _ = sim.discard_constructed_limbo(plane, Some(rules));
        return false;
    }
    let now = sim.session.binary_frame;
    if let Some(entity) = sim.substrate.entities.get_mut(plane) {
        crate::sim::mission::authority::commence_entity_mission(entity, now);
    }
    true
}
