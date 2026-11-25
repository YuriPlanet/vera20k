//! The single Foot Rescue mission owner (gamemd.exe 0x004DDF90).
//!
//! Unit and Infantry dispatch to this body (both concrete Rescue virtuals
//! dispatch to `0x004DDF90`, tools/spatial_oracle/anytown_damage/
//! foot_missions), and AircraftClass keeps the same address in its own
//! Rescue slot (`vt+0x258` holds `4DDF90` in the retail table at
//! `0x007E22A4`), so an aircraft runs it as well
//! (`aircraft::dispatch_mission`).
//!
//! Its direct Greatest_Threat uses the archive's +48 XYZ, no passive timer or
//! health debit. Admission is strict distance < mode1 range*1.5. Empty results
//! clear 688, set status 1, use the House500200 home owner, assign a
//! destination, then clear the archive before the current-mission cadence
//! draw. Status 1 without NavCom queues AreaGuard and calls Commence
//! directly.
//!
//! Native execution: foot_missions Rescue rows and house_base_return.
//! With a Target in state 0 it approaches through
//! [`Simulation::approach_target`] (`0x004DDFEB`).
//!
//! The chain's RNG draws, timer writes and detach calls: the mission draws
//! no random number of its own; the dispatch epilogue draws the cadence
//! jitter (`mission::authority::rate_epilogue`, `rate*900+RandomRanged(0,2)`)
//! and writes the mission timer, and the Rescue body assigns the archive
//! target (the detach of the previous target) at `0x004DDFEB`.
//!
//! RESIDUALS, both about an aircraft on this mission:
//! - The approach step (`0x004DDFEB`) calls the receiver's own Approach
//!   virtual. [`Simulation::approach_target`] represents the Foot body, which
//!   refuses an aircraft (`movement::foot_approach::owns_foot_approach`), so
//!   the aircraft does not close on its Target. Trigger: an aircraft on
//!   Rescue with a Target. Effect: the aircraft holds its position instead of
//!   approaching. Frequency: VERA's only Rescue producer, the base-defence
//!   response, queues it on infantry and units. Downstream risk: an aircraft
//!   given Rescue by a map or trigger never reaches its Target.
//! - The home destination comes from House500200 (Find_Passable_Cell_Near_Unit).
//!   VERA ported its Unit/Infantry arm (`world::house_base::house_return_cell`);
//!   the other variants of that body are not ported, so the aircraft's
//!   destination is cleared instead of set to its house's home. Trigger: an
//!   aircraft on Rescue. Effect: the aircraft does not fly home and reaches
//!   the AreaGuard arm one visit earlier. Frequency: as above. Downstream
//!   risk: the aircraft guards where it stands.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::mission::authority::{LiveReadyInputProvider, rate_epilogue};
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::{FrameEffects, Simulation};

/// `FootClass::Mission_Rescue @ 0x004DDF90`. The MissionClass dispatcher
/// owns the returned delay timer.
pub(crate) fn mission_rescue(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    id: u64,
    effects: &mut FrameEffects<'_>,
) -> i32 {
    let Some(actor) = sim.substrate.entities.get(id) else {
        return 1;
    };
    let state = actor.mission.handler_state();
    if state == 1 && actor.navigation.nav_com().is_none() {
        sim.substrate
            .entities
            .get_mut(id)
            .expect("Rescue receiver")
            .set_archive_target(None);
        let _ = sim.mission_queue_exact(
            id,
            MissionId::from_known(MissionType::AreaGuard),
            0,
            sim.session.binary_frame(),
            &LiveReadyInputProvider { rules },
        );
        let _ = sim
            .substrate
            .mission_commence_exact(id, sim.session.binary_frame());
    } else if state == 0 && actor.attack_target.target().is_none() {
        if actor.archive_target().is_none() {
            let post = crate::sim::movement::ground_pose::physical_cell_target(sim, id);
            sim.substrate
                .entities
                .get_mut(id)
                .expect("Rescue receiver")
                .set_archive_target(post);
        }
        sim.substrate
            .entities
            .get_mut(id)
            .expect("Rescue receiver")
            .seldom
            .foot_retarget_after_stop
            .clear_for_rescue();
        let archive = sim
            .substrate
            .entities
            .get(id)
            .and_then(|actor| actor.archive_target());
        let coords = |sim: &Simulation, target| {
            crate::sim::movement::ground_pose::target_get_coords(
                target,
                sim,
                sim.resolved_terrain
                    .as_ref()
                    .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                    .as_ref(),
            )
            .map(|coord| [coord.x, coord.y, coord.z])
        };
        let point = archive.and_then(|post| coords(sim, post));
        let pick = point.and_then(|point| {
            sim.greatest_threat_represented(
                rules,
                registry,
                id,
                crate::sim::combat::ScanMission::Hunt,
                Some(point),
            )
        });
        if let (Some(point), Some(pick)) = (point, pick)
            && let Some(target) = coords(sim, pick)
        {
            //1.5 is exact for the native integer range, so this signed
            //scaled comparison preserves its strict FCOMP result without
            //rounding a fractional threshold or discounting foundations.
            let distance = crate::util::native_x87::object_distance(point, target, None);
            let range = crate::sim::combat::threat_range::object_threat_range(sim, id, rules);
            if i64::from(distance) * 2 < i64::from(range) * 3 {
                let _ = sim.assign_target_represented(id, Some(pick), rules, effects);
            }
        }
        if sim
            .substrate
            .entities
            .get(id)
            .is_some_and(|actor| actor.attack_target.target().is_some())
        {
            return 1;
        }
        sim.substrate
            .entities
            .get_mut(id)
            .expect("Rescue receiver")
            .mission
            .set_handler_state(1);
        let owner = sim
            .substrate
            .entities
            .get(id)
            .expect("Rescue receiver")
            .owner();
        let destination = sim
            .house_return_cell(owner, id)
            .map(|(rx, ry)| NavTargetRef::cell(rx, ry));
        let _ = sim.assign_destination_represented(id, destination, rules, effects);
        sim.substrate
            .entities
            .get_mut(id)
            .expect("Rescue receiver")
            .set_archive_target(None);
    } else if state == 0 {
        // `0x004DDFDB..0x004DDFEB`: with a Target, approach it.
        sim.approach_target(id, rules, registry, effects);
    }
    let mission = sim
        .substrate
        .entities
        .get(id)
        .and_then(|actor| actor.mission.current().known())
        .unwrap_or(MissionType::Rescue);
    rate_epilogue(rules, mission, &mut sim.scenario_rng)
}
