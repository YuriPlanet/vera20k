//! Map loading, terrain, tile system, and theater management.
//!
//! RA2 maps are INI files with binary-encoded sections:
//! - [Map] — metadata (size, theater name)
//! - [IsoMapPack5] — base64-encoded terrain tile data
//! - [OverlayDataPack] — base64-encoded overlay data (ore, gems, walls)
//! - [Structures] / [Units] / [Infantry] — entity placements
//!
//! Each map specifies a theater (temperate, snow, urban, etc.) which determines
//! which terrain tileset (.tmp files) and palette to use.
//!
//! ## Dependency rules
//! - map/ depends on: assets/ (reads .tmp terrain tiles), rules/ (terrain type definitions)
//! - map/ does NOT depend on: sim/, render/, ui/, audio/, net/
//! - trigger_runtime (runtime evaluation) lives in sim/ — map/ only holds static definitions

pub mod actions;
pub(crate) mod authored_overlay;
pub mod basic;
pub mod bridge_facts;
pub mod bridge_pavement;
pub mod bridge_rim_tiles;
pub mod cell_index;
pub(crate) mod construction_trace;
pub mod scenario_menu;
pub mod scenario_sources;
pub mod skirmish_scenarios;
pub mod source;
pub mod cell_tags;
pub mod entities;
pub mod events;
pub mod houses;
pub(crate) mod iso_tile_flood;
pub mod lat;
pub mod lighting;
pub mod map_file;
pub mod overlay;
pub mod playfield;
pub mod preview;
pub mod retail_trig;
pub mod resolved_terrain;
pub mod rmg;
pub mod tags;
pub mod terrain;
pub mod theater;
pub(crate) mod tiberium_cell;
pub mod tile_variant_selector;
pub mod trigger_graph;
pub mod triggers;
pub mod tube_facts;
pub mod tubes;
pub mod variable_names;
pub mod waypoints;
pub(crate) mod voxel_slope;
// Future modules — uncomment as implemented:
// pub mod cell;

pub mod voxel_pose;
