//! Minimal production system: credits, build queue, and unit spawning.
//!
//! This is a first playable loop implementation. Split into sub-modules:
//! - `production_types`: shared types, constants, state containers
//! - `factory`: queue and per-step charging kernels
//! - `factory_lifecycle`: held-object birth, completion, cancellation and release
//! - `production_queue`: queue views and completed mobile delivery
//! - `production_economy`: resource harvesting and credit delivery
//! - `production_placement`: building placement
//! - `production_sell`: building sale and repair
//! - `production_tech`: tech tree, build options, factory matching, spawn cells

mod factory;
mod factory_lifecycle;
mod production_economy;
mod production_placement;
mod production_queue;
mod production_refinery;
mod production_sell;
mod production_spawn;
mod production_tech;
mod production_types;
mod wall_placement;
mod war_factory_exit;

// Re-export everything so external code can still use `production::X`.
pub use self::factory::{
    BuildEligibility, BuildStepTimeInputs, CancelOutcome, Factory, FactoryRegistry, FactoryView,
    PRODUCTION_STEPS, PendingObject, STEP_RATE_MAX, STEP_RATE_MIN, SpecialItem, StepOutcome,
    build_step_time, category_for_object,
};
pub use self::factory_lifecycle::{cancel_by_type_for_owner, cancel_last_for_owner, enqueue_by_type};
pub(crate) use self::factory_lifecycle::{FactoryRestoreError, validate_restored_factory_state};
pub use self::production_economy::is_harvester_type;
pub use self::production_placement::{
    active_producer_for_owner_category, cycle_active_producer_for_owner_category,
    place_ready_building_with_overlays, place_ready_building_without_overlays,
    placement_preview_for_owner_with_overlays, placement_preview_for_owner_without_overlays,
    toggle_pause_for_owner_category,
};
pub use self::production_queue::{
    build_options_for_owner, credits_for_owner, enqueue_default_unit_for_owner,
    has_strict_build_option_for_owner, power_balance_for_owner, queue_view_for_owner,
    rally_point_for_owner, ready_buildings_for_owner, set_rally_point_for_owner,
    theoretical_power_for_owner, tick_production, tick_production_with_overlay_registry,
};
pub(crate) use self::production_refinery::spawn_completed_refinery_free_units;
pub(crate) use self::production_sell::{
    archive_less_sale, begin_selling, building_type_refund, eject_destruction_garrison_with_context,
    eject_red_hp_garrison, sell_complete, sell_stage_one, sell_stage_zero, undeploy_target,
};
#[cfg(test)]
pub(crate) use self::production_sell::{eject_destruction_garrison, sell_building_now_for_test};
pub use self::production_sell::{
    SellOrder, can_sell_building, sell_back, tick_repairs, toggle_repair,
};
pub use self::production_spawn::find_spawn_cell_for_owner;
pub use self::production_tech::{
    building_base_foundation_cells, building_footprint_cells, building_movement_blocking_cells,
    building_movement_blocking_cells_for_state, foundation_dimensions, is_matching_factory,
    producer_candidates_for_owner_category, structure_satisfies_prerequisite,
};
pub use self::production_types::*;
pub use self::war_factory_exit::tick_war_factory_exit_contacts;

// Re-exports for external consumers (files outside production/ that previously
// imported private submodules directly).
pub(in crate::sim) use self::factory_lifecycle::revalidate_and_step_factories;
#[cfg(test)]
pub(in crate::sim) use self::factory_lifecycle::construct_active_factory_fixture;
pub(in crate::sim) use self::production_queue::credits_entry_for_owner;
pub(in crate::sim) use self::production_spawn::produced_unit_unlimbo_entry_at_resolved_cell;

// Re-exports used by test sub-modules (via `super::` in test files).
#[cfg(test)]
pub(in crate::sim) use self::production_tech::{
    build_time_base_frames, effective_progress_rate_ppm_for_type,
    effective_time_to_build_frames_for_type,
};

#[cfg(test)]
#[path = "production_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "production_queue_tests.rs"]
mod queue_tests;

#[cfg(test)]
#[path = "production_placement_tests.rs"]
mod placement_tests;

#[cfg(test)]
#[path = "production_replay_tests.rs"]
mod replay_tests;

#[cfg(test)]
#[path = "factory_lifecycle_tests.rs"]
mod lifecycle_tests;
