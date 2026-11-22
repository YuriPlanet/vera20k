//! Armory service: a building with `Armory=yes` promotes every infantry that
//! enters it to Elite (`VeterancyStruct::SetElite(1) @ 0x007500B0`) and spends
//! one round of its Ammo doing so.
//!
//! Native references in game.exe:
//! - `BuildingTypeClass::ReadINI` reads `Armory` at `0x0045A3BB` into
//!   BuildingType+0x135C.
//! - `BuildingClass::MissionRepairAndProduce` groups ConstructionYard,
//!   Hospital, Armory, UnitRepair and UnitReload in the same dispatch
//!   (`0x0044C836..0x0044C970`); the Armory family is otherwise dormant in
//!   retail rules because no stock BuildingType sets the flag.
//! - Armories never auto-reload: `building_missions::refills` already excludes
//!   them, matching the native "Armory prevents Ammo from reloading" rule.
//!
//! The promotions left are the building's signed Ammo. A missing `Ammo=` is
//! copied as -1 and the game uses its absolute value, so an Armory without an
//! explicit Ammo promotes exactly one soldier. `Ammo=0` promotes none.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::passenger::{DepartureRoute, depart_cargo_head};
use crate::sim::world::{FrameEffects, Simulation};
use crate::util::native_x87::{
    MaskedX87Chop53 as X87, MaskedX87Ordering as Ordering, NativeF64Bits,
};

/// How many more infantry this Armory can promote. The native Armory takes
/// the absolute value of the signed Ammo count, so an absent `Ammo=` (-1)
/// leaves one promotion.
pub(crate) fn promotions_remaining(building: &GameEntity) -> i32 {
    building.ammo().map_or(0, |ammo| {
        let current = ammo.current();
        if current < 0 { 1 } else { current }
    })
}

/// Spend one promotion. A positive Ammo decrements; the constructor default
/// -1 becomes 0 so the one implicit promotion is used up as well.
fn spend_promotion(building: &mut GameEntity) {
    if let Some(ammo) = building.ammo_mut() {
        match ammo.current() {
            current if current > 0 => {
                ammo.add(-1);
            }
            current if current < 0 => {
                ammo.add(1);
            }
            _ => {}
        }
    }
}

/// The Armory arm of `BuildingClass::MissionRepairAndProduce`. Its cadence is
/// the same Repair stage as the Hospital arm; on completion the cargo head is
/// promoted and exits through the shared produced-object exit.
pub(super) fn mission_repair(
    sim: &mut Simulation,
    rules: &RuleSet,
    armory: u64,
    registry: Option<&OverlayTypeRegistry>,
    effects: &mut FrameEffects<'_>,
) -> i32 {
    let now = sim.session.binary_frame();
    let Some(building) = sim.substrate.entities.get_mut(armory) else {
        return 1;
    };
    let state = building.mission.handler_state();
    if state == 0 {
        building.mission.set_handler_state(2);
        building.mission_leaf.set_building_ready_latch(0);
        building
            .mission_leaf
            .start_building_repair_progress(now as i32);
        spend_promotion(building);
        return 1;
    }
    if state != 2 {
        return 1;
    }
    let progress = building
        .mission_leaf
        .advance_building_repair_progress(now as i32);
    let threshold = X87::mul(
        X87::load_f64(NativeF64Bits::from_bits(
            rules.general.infantry_repair_rate.to_bits(),
        )),
        X87::load_i32(900),
    );
    if X87::compare(threshold, X87::load_i32(progress)) == Ordering::Greater {
        return 1;
    }
    building.mission_leaf.set_building_ready_latch(0);
    building.mission_leaf.reset_building_repair_progress();
    // The Hospital route pops Cargo's head and ignores a refused exit, which
    // is the shape the native Armory's shared exit uses. We deliberately do
    // not reproduce the retail "trainee sticks at the upper-left corner"
    // bug: a refused exit still leaves the promoted soldier removed.
    let _ = depart_cargo_head(
        sim,
        rules,
        armory,
        DepartureRoute::Hospital,
        |sim, trainee, effects| {
            if let Some(entity) = sim.substrate.entities.get_mut(trainee) {
                entity.veterancy.set_elite();
            }
            let _ = crate::sim::production::exit_produced_object(
                sim,
                rules,
                armory,
                trainee,
                (0, 0),
                registry,
                effects,
            );
            Ok(())
        },
        effects,
    );
    let _ = sim.mission_queue_exact(
        armory,
        MissionId::from_known(MissionType::Guard),
        0,
        now,
        &LiveReadyInputProvider { rules },
    );
    1
}
