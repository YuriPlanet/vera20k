//! Instance builders — per-frame SpriteInstance generation for entities and overlays.
//!
//! Split into sub-modules for the 600-line file limit.
//! This facade re-exports all public items so external consumers
//! can continue using `crate::app::presentation::instances::*`.
//!
//! ## Sub-modules
//! - `helpers` — depth sorting, interpolation, visibility checks
//! - `units` — voxel unit instance generation (turrets, harvest overlays)
//! - `shp` — SHP building/infantry instances (building anims, bibs)
//! - `overlays` — map overlays, muzzle flashes, fog snapshots
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

mod helpers;
pub(crate) use helpers::*;
mod foot_depth;

mod units;
pub(crate) use units::*;

mod shp;
pub(crate) use shp::*;

mod overlays;
pub(crate) use overlays::*;

mod particles;
pub(crate) use particles::*;

pub mod bridges;
