//! The single Foot Enter mission owner (gamemd.exe 0x004D9290).
//!
//! Refinery and repair-depot entry share contact/archive admission, pending
//! entry parking, navigation handoff and the current-mission Rate/RNG tail.
//! The class radio receiver owns docking decisions; this mission does not
//! repeat them. Evidence: tools/spatial_oracle/refinery_dock.json and .md.
//! Infantry inherit the body (InfantryClass vt+0x240); theirs runs once the
//! Boarding adapter has let them go (`techno_ai::mission_handlers`).
//!
//! RESIDUAL: the nav-queue and `Teleporter=` arms take the class destination
//! setter (vt+0x480); only the Unit setters are represented, so an
//! infantryman skips them. VERA's infantry reach that arm only with a radio
//! contact or an archived Techno on Enter, which the Boarding adapter never
//! gives them.

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
    let now = sim.session.binary_frame;
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
                .and_then(|entity| entity.navigation.nav_com);
            let nav_is_mover = match nav {
                Some(
                    NavTargetRef::Entity { id: nav }
                    | NavTargetRef::Object { id: nav }
                    | NavTargetRef::Building { id: nav },
                ) => sim.substrate.entities.get(nav).is_some_and(|other| {
                    matches!(
                        other.category,
                        EntityCategory::Unit | EntityCategory::Aircraft
                    )
                }),
                _ => false,
            };
            if !parked && !nav_is_mover {
                crate::sim::world::enter_idle_mode(sim, id, rules, registry, effects);
            }
            let _ = sim.mission_commence_exact(id, now);
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
                .is_some_and(|entity| entity.dock_entered_with.is_some());
            let unit = sim
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.category == EntityCategory::Unit);
            if reply != RadioResponse::Roger && !tethered {
                // 0x004D92CE..0x004D92E8.
                radio::transmit_to_contact(sim, id, RadioMessage::Break, Some(rules), effects);
                crate::sim::world::enter_idle_mode(sim, id, rules, registry, effects);
            } else if unit && !pop_nav_queue(sim, rules, id, effects) {
                teleporter_reassign(sim, rules, id, effects);
            }
        }
    }
    // 0x004D946C..0x004D9497: the CURRENT mission's Rate (a Commence above may
    // have changed it).
    sim.mission_rate_epilogue_for(rules, id, MissionType::Enter)
}

/// `0x004D92ED..0x004D93E3`: with no NavCom and a waypoint queued, a
/// piggyback that `Is_Ok_To_End` allows ends first (`0x004D9309..0x004D937C`,
/// no `Is_Piggybacking` test), then the class setter takes `NavQueue[0]` with
/// the flag 0 (the queue is kept) and the first entry is deleted. Only Cell
/// waypoints are represented. Returns whether this arm ran.
fn pop_nav_queue(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    effects: FrameEffects<'_>,
) -> bool {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    if entity.navigation.nav_com.is_some() || entity.navigation.nav_queue.is_empty() {
        return false;
    }
    let _ = sim.try_end_piggyback(id, Some(rules));
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    let queue = entity.navigation.nav_queue.clone();
    if let NavTargetRef::Cell { rx, ry } = queue[0] {
        sim.set_unit_destination(
            id,
            crate::sim::components::NavTargetRef::cell(rx, ry),
            rules,
            false,
            effects,
        );
    }
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.navigation.nav_queue = queue[1..].to_vec();
    }
    true
}

/// `0x004D93E8..0x004D941D`: a `Teleporter=` type drops NavCom and NavComAux
/// raw and hands the old NavCom back to the class setter with the flag 1. On
/// the pad approach that re-runs the Teleporter arm and the Teleport Move_To;
/// after the warp NavCom is NULL and the setter returns at once (`0x00741A80`).
fn teleporter_reassign(sim: &mut Simulation, rules: &RuleSet, id: u64, effects: FrameEffects<'_>) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    if !sim
        .object_type(entity.type_ref(), rules)
        .is_some_and(|object| object.teleporter)
    {
        return;
    }
    let nav = entity.navigation.nav_com;
    if !matches!(nav, None | Some(NavTargetRef::Cell { .. })) {
        return;
    }
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        crate::sim::movement::foot_stop_moving(entity);
    }
    match nav {
        Some(NavTargetRef::Cell { rx, ry }) => {
            sim.set_unit_destination(
                id,
                crate::sim::components::NavTargetRef::cell(rx, ry),
                rules,
                true,
                effects,
            );
        }
        _ => {
            sim.set_unit_null_destination(id, Some(rules), None, effects);
        }
    }
}
