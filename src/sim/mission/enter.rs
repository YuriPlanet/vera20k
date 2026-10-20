//! The single Foot Enter mission owner (gamemd.exe 0x004D9290).
//!
//! Refinery, repair-depot, transport and absorber entry share
//! contact/archive admission, pending entry parking, navigation handoff and
//! the current-mission Rate/RNG tail. The class radio receiver owns docking
//! decisions (a transport's DOCKING is `radio::receive`'s Unit 0x0E arm);
//! this mission does not repeat them. Evidence:
//! tools/spatial_oracle/refinery_dock.json and .md. Infantry inherit the body
//! (InfantryClass vt+0x240; `techno_ai::mission_handlers`). The
//! nav-queue and `Teleporter=` arms take the receiver's class setter
//! (vt+0x480, [`Simulation::assign_destination_represented`]) whatever its
//! class: no class test precedes them (`0x004D92C2..0x004D93FA`).

use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::mission::MissionType;
use crate::sim::radio::{self, RadioMessage, RadioPayload, RadioResponse};
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;

/// `FootClass::Mission_Enter @ 0x004D9290`, shared by refinery and depot
/// callers and infantry. The MissionClass dispatcher owns the returned delay
/// timer. The idle exits run the class receiver (vt+0x484,
/// [`crate::sim::world::enter_idle_mode`]).
pub(crate) fn mission_enter(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    id: u64,
    effects: FrameEffects<'_>,
) -> i32 {
    let now = sim.session.binary_frame();
    let Some(entity) = sim.substrate.entities.get(id) else {
        return 1;
    };
    // 0x004D9294..0x004D92AC: Contacts[0], else the Techno behind ArchiveTarget.
    let target = entity
        .radio_contacts
        .slot(0)
        .or(match entity.archive_target() {
            Some(crate::sim::combat::TargetKind::Entity(archived)) => Some(archived),
            _ => None,
        });
    match target {
        None => {
            // 4D9425..9466: pending entry may queue Move. Otherwise an
            // object destination to a Unit/Aircraft suppresses idle. Every
            // path commences before sampling the CURRENT mission's Rate.
            let parked =
                crate::sim::docking::building_dock::park_pending_entry(sim, rules, id, effects);
            let nav = sim
                .substrate
                .entities
                .get(id)
                .and_then(|entity| entity.navigation.nav_com());
            let nav_is_mover = match nav {
                Some(NavTargetRef::Object { id: nav }) => {
                    sim.substrate.entities.get(nav).is_some_and(|other| {
                        matches!(
                            other.category(),
                            EntityCategory::Unit | EntityCategory::Aircraft
                        )
                    })
                }
                _ => false,
            };
            if !parked && !nav_is_mover {
                crate::sim::world::enter_idle_mode(sim, id, rules, registry, effects);
            }
            let _ = sim.substrate.mission_commence_exact(id, now);
        }
        Some(target) => {
            let reply = radio::transmit(
                sim,
                id,
                target,
                RadioMessage::CanDock,
                RadioPayload::default(),
                Some(rules),
                effects,
            );
            let tethered = sim
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.tether.partner().is_some());
            if reply != RadioResponse::Roger && !tethered {
                // 0x004D92CE..0x004D92E8.
                radio::transmit_to_contact(sim, id, RadioMessage::Break, Some(rules), effects);
                crate::sim::world::enter_idle_mode(sim, id, rules, registry, effects);
            } else if !pop_nav_queue(sim, rules, registry, id, effects) {
                teleporter_reassign(sim, rules, registry, id, effects);
            }
        }
    }
    // 0x004D946C..0x004D9497: the CURRENT mission's Rate (a Commence above may
    // have changed it).
    Simulation::mission_rate_epilogue_for(
        &mut sim.scenario_rng,
        &sim.substrate,
        rules,
        id,
        MissionType::Enter,
    )
}

/// `0x004D92ED..0x004D93E3`: with no NavCom and a waypoint queued, a
/// piggyback that `Is_Ok_To_End` allows ends first (`0x004D9309..0x004D937C`,
/// no `Is_Piggybacking` test), then the class setter takes `NavQueue[0]` with
/// the flag 0 (the queue is kept) and the first entry is deleted. Returns
/// whether this arm ran.
fn pop_nav_queue(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    id: u64,
    effects: FrameEffects<'_>,
) -> bool {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    if entity.navigation.nav_com().is_some() || entity.navigation.nav_queue().is_empty() {
        return false;
    }
    let _ = sim.try_end_piggyback(id, Some(rules));
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    let first = entity.navigation.nav_queue()[0];
    // 0x004D939A: vt+0x480(NavQueue[0], 0).
    let _ = sim.assign_destination_keeping_queue(id, Some(first), Some(rules), registry, effects);
    // The shift deletes the first entry of the queue the setter left; the
    // UnitRepair arm's modified NULL (`0x007423CA`) may have emptied it.
    if let Some(entity) = sim.substrate.entities.get_mut(id)
        && !entity.navigation.nav_queue().is_empty()
    {
        entity.navigation.pop_nav_queue_front();
    }
    true
}

/// `0x004D93E8..0x004D941D`: a `Teleporter=` type (TechnoType+0xCD4) drops
/// NavCom and NavComAux raw and hands the old NavCom, whatever it names, back
/// to the class setter with the flag 1. On a Unit's pad approach that re-runs
/// the Teleporter arm and the Teleport Move_To; after the warp NavCom is NULL
/// and the setter returns at once (`0x00741A80`).
fn teleporter_reassign(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    id: u64,
    effects: FrameEffects<'_>,
) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    if !sim
        .object_type(entity.type_ref(), rules)
        .is_some_and(|object| object.teleporter)
    {
        return;
    }
    let nav = entity.navigation.nav_com();
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.navigation.stop_moving();
    }
    let _ = sim.assign_destination_represented(id, nav, Some(rules), registry, effects);
}
