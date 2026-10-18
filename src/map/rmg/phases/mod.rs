//! Generation phase bodies, one module per pipeline stage.
//!
//! Each phase is a plain function over the grid/scratch/rng owners and
//! commits state in the original's order. The stage sequencing itself lives
//! in `super::generate`.

pub mod adjacency;
pub mod area;
pub mod blob;
pub mod bridge;
pub mod bridge_deck;
pub mod carve;
pub mod carve_driver;
pub(super) mod cliff_materialization;
pub(super) mod cliff_variants;
pub mod connector;
pub mod green_spread;
pub mod hills;
pub mod hills_corners;
pub mod island_passes;
pub mod lake;
pub mod lat_fixup;
pub mod lat_patches;
pub mod meander;
pub mod ramp;
pub mod regions;
pub mod river;
pub mod rocks;
pub mod shore;
pub mod starts;
pub mod tech_buildings;
pub mod tiberium;
pub mod trees;
pub mod water;
pub mod water_finalize;
pub mod zones;

#[cfg(test)]
mod cliff_native_tests;
