//! Pathfinding system — A* search, zone connectivity, terrain speed, and path smoothing.
//!
//! Combines cell-level A* with zone-aware hierarchical search for fast unreachability
//! detection and hierarchy-marked pruning.
//!
//! TODO(RE): The current split is still only partially aligned with RA2/YR. Zone
//! rebuilds use the recovered nodeIndex -> zoneId semantics, but bridge-layer remap,
//! full hierarchical subzone support, and the exact regular-vs-hierarchical path entry
//! behavior are still pending.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on map/ (terrain, resolved_terrain).
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

// Core A* algorithm, PathGrid, PathCell
mod core;
// Search scratch shared by the cell A* and the zone precheck
mod native_heap;
mod search_table;

// Path post-processing
pub mod path_smooth;

// Terrain and movement costs
pub mod passability;
pub mod terrain_speed;

// Zone connectivity (flood-fill zones, hierarchy, zone-aware search)
pub(crate) mod zone_build;
pub mod zone_hierarchy;
pub(crate) mod zone_incremental;
pub mod zone_map;
pub mod zone_search;

// Re-export core types so external code uses crate::sim::pathfinding::PathGrid etc.
pub use self::core::*;

#[cfg(test)]
#[path = "zone_map_tests.rs"]
mod zone_map_tests;
