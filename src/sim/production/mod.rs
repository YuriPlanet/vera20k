//! Minimal production system: credits, build queue, and unit spawning.
//!
//! This is a first playable loop implementation. Split into sub-modules:
//! - `production_types`: shared types, constants, state containers
//! - `can_build`: CanBuild, FindFactory and CheckBuildLimit, for every house
//! - `factory`: queue and per-step charging kernels
//! - `factory_lifecycle`: held-object birth, completion, cancellation and release
//! - `factory_ai`: a computer house's production at its own factory buildings
//! - `production_queue`: queue views and completed mobile delivery
//! - `production_spawn`: the ExitObject arm a mobile product leaves through
//! - `production_placement`: building placement
//! - `production_repair`: building repair and the computer's low-credit sale
//! - `production_sell`: building sale
//! - `production_tech`: the player's build options and factory matching

mod can_build;
mod factory;
mod factory_ai;
mod factory_lifecycle;
mod production_placement;
mod production_queue;
mod production_refinery;
mod production_repair;
pub(crate) use production_repair::{AiSellRepair, RepairByte};
mod production_sell;
mod production_spawn;
mod production_tech;
mod production_types;
mod wall_placement;

// Re-export everything so external code can still use `production::X`.
pub use self::factory::{
    CancelOutcome, Factory, FactoryHolder, FactoryRegistry, FactoryView, PRODUCTION_STEPS,
    PendingObject, STEP_RATE_MAX, STEP_RATE_MIN, StepOutcome, TimeToBuildInputs,
    category_for_object, time_to_build, type_base_build_time,
};
pub(crate) use self::factory_lifecycle::enqueue_selected;
pub(crate) use self::factory_lifecycle::suspend_production;
pub use self::factory_lifecycle::{
    FactoryRestoreError, ready_building_entity_for_owner, validate_restored_factory_state,
};
pub use self::factory_lifecycle::{
    ProductionCancellation, cancel_production_for_owner, enqueue_by_type,
};
pub(crate) use self::production_placement::place_production_with_overlays;
pub use self::production_placement::{
    active_producer_for_owner_category, cycle_active_producer_for_owner_category,
    is_within_build_area, placement_preview_for_owner_with_overlays,
};
#[cfg(any(test, feature = "test-support"))]
pub use self::production_queue::{
    dispatch_production_changes_for_tests, publish_production_changes_for_tests,
};
#[cfg(any(test, feature = "test-support"))]
pub(in crate::sim) use self::production_queue::house_for_test;
pub use self::production_queue::{
    build_options_for_owner, credits_for_owner, has_build_option_for_owner,
    power_balance_for_owner, publish_production_changes, queue_view_for_owner,
    ready_buildings_for_owner,
};
pub(crate) use self::production_refinery::spawn_building_free_unit;
pub use self::production_repair::can_repair_building;
pub use self::production_repair::{RepairControl, toggle_repair};
pub(crate) use self::production_repair::{
    engineer_repair, repair_step_cost, update_repair_and_power,
};
#[cfg(any(test, feature = "test-support"))]
pub(crate) use self::production_sell::begin_selling;
pub(crate) use self::production_sell::sell_back;
#[cfg(any(test, feature = "test-support"))]
pub use self::production_sell::sell_building_now_for_test;
pub use self::production_sell::sell_stage_zero;
pub use self::production_sell::{SellOrder, can_sell_building};
pub(crate) use self::production_sell::{
    archive_less_sale, sell_after_owner_change, sell_complete, sell_stage_one, techno_refund,
    type_refund, undeploy_target,
};
pub use self::production_spawn::mark_war_factory_spawn_contact;
pub use self::production_tech::{
    CameoDrawState, all_build_options_for_owner, cameo_draw_state, construction_options_for_owner,
    initial_construction_options,
};
pub use self::production_tech::{
    building_base_foundation_cells, building_movement_blocking_cells,
    producer_candidates_for_owner_category,
};
pub use self::production_types::*;

// Re-exports for external consumers (files outside production/ that previously
// imported private submodules directly).
pub(crate) use self::can_build::{
    CanBuild, can_build, initialize_factory_primary, toggle_factory_primary,
};
pub use self::can_build::{can_select_factory_primary, find_factory, object_find_factory};
pub(crate) use self::factory_ai::{
    AiPlacementTimer, detach_all as detach_building_factory, factory_ai,
};
#[cfg(any(test, feature = "test-support"))]
pub use self::factory_lifecycle::complete_held_building_for_test;
#[cfg(any(test, feature = "test-support"))]
pub(in crate::sim) use self::factory_lifecycle::construct_active_factory_fixture;
#[cfg(any(test, feature = "test-support"))]
pub(in crate::sim) use self::factory_lifecycle::release_delivered_mobile;
pub(in crate::sim) use self::factory_lifecycle::{
    abandon_house_factories, detach_building_production, recalculate_strips,
    refresh_factory_rates_for_house, step_factories, update_factory_queue,
};
pub(in crate::sim) use self::production_queue::exit_produced_object;
pub(crate) use self::production_tech::factory_category_for_building;
pub(crate) use self::wall_placement::{
    autofill_walls, linked_overlay_id as linked_wall_overlay_id,
};

#[cfg(test)]
#[path = "production_tests.rs"]
mod tests;

#[cfg(test)]
mod industrial_plant_retail_tests;

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

#[cfg(test)]
mod manual_primary_tests;

#[cfg(test)]
mod native_event_tests;
