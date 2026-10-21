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
//! - `overlays` — map overlays, AnimClass objects, fog snapshots
//! - `cell_bodies` — the one native sweep order for every overlay body
//! - `voxel_anims` — VoxelAnimClass debris (tumbling VXL pieces)
//! - `tether_redraw` — an unloading building's redraw of its tethered contacts
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

mod helpers;
pub(crate) use helpers::*;
mod anim_draw;
#[cfg(test)]
mod anim_translucency_tests;
mod foot_depth;

mod units;
pub(crate) use units::*;

mod shp;
pub(crate) use shp::*;

mod overlays;
pub(crate) use overlays::*;

mod cell_bodies;
pub(crate) use cell_bodies::*;

mod projectiles;
pub(crate) use projectiles::*;

mod particles;
pub(crate) use particles::*;

mod voxel_anims;
pub(crate) use voxel_anims::*;

mod tether_redraw;
pub(crate) use tether_redraw::*;

pub mod bridges;
