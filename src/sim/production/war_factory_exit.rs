//! War-factory exit radio-contact transient break.
//!
//! A newborn land vehicle from a war factory holds a live radio contact with its
//! producer so it can drive across the factory footprint (the NumberImpassableRows
//! row-skip read in `movement_occupancy::building_entry_skip_cells`). The producer reproduces
//! gamemd by breaking that contact the moment the vehicle's per-cell process finds
//! no building under its current cell (footprint cleared). Despawn / limbo cleanup
//! (`clear_radio_contacts_for`) remains the safety net. sim/ only — depends on
//! map/entities, rules, and sim::{entity_store,intern,movement::locomotor,occupancy}.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::StringInterner;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::OccupancyGrid;

use super::production_spawn::exact_land_vehicle_exit_factory;

/// Break each war-factory exit contact whose vehicle has cleared the factory
/// footprint. Runs once per tick, right after ground movement.
///
/// Gates (all must hold), matching gamemd's per-cell-process break:
/// - the mover is a vehicle carrying a dock-entered flag (`+0x418`) toward a producer;
/// - that producer is a WeaponsFactory land-vehicle exit factory (the refinery's
///   dock-entered flag points at a refinery -> skipped, so its lifecycle is intact);
/// - the mover's current cell has no `Structure` occupant (footprint cleared).
pub fn tick_war_factory_exit_contacts(
    entities: &mut EntityStore,
    occupancy: &OccupancyGrid,
    rules: &RuleSet,
    interner: &StringInterner,
) {
    // Pass 1 (immutable reads): decide which (mover, producer) contacts to break.
    // `entities.values()` iterates in stable_id order -> deterministic.
    let to_break: Vec<(u64, u64)> = {
        let ents: &EntityStore = entities;
        ents.values()
            .filter_map(|mover| {
                // Skip Dying corpses (mover or its producer) awaiting the
                // end-of-tick drain — don't compute contact breaks against them.
                if mover.dying || mover.category != EntityCategory::Unit {
                    return None;
                }
                let producer_id = mover.dock_entered_with?;
                let producer = ents.get(producer_id)?;
                if producer.dying || producer.category != EntityCategory::Structure {
                    return None;
                }
                if !exact_land_vehicle_exit_factory(rules, interner.resolve(producer.type_ref())) {
                    return None;
                }
                let on_footprint = occupancy
                    .get(mover.position.rx, mover.position.ry)
                    .is_some_and(|cell| {
                        cell.blockers(MovementLayer::Ground).any(|id| {
                            ents.get(id)
                                .is_some_and(|o| o.category == EntityCategory::Structure)
                        })
                    });
                if on_footprint {
                    return None;
                }
                Some((mover.stable_id(), producer_id))
            })
            .collect()
    };

    // Pass 2 (mutable): apply the break (models 0x08 -> 0x19 -> 0x03).
    for (mover_id, producer_id) in to_break {
        if let Some(mover) = entities.get_mut(mover_id) {
            mover.clear_live_contact_with(producer_id);
            mover.dock_entered_with = None;
        }
    }
}
