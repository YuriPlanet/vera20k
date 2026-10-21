//! The sidebar radar (`RadarClass`): the generated terrain surface and its
//! geometry, the retained object tracker, the client-local event array, the
//! housing animation, and the minimap renderer that draws them.

pub mod anim;
mod current_cell;
mod events;
pub(super) mod geometry;
mod helpers;
mod interaction;
pub(crate) use interaction::{NativeRadarCameraTarget, NativeRadarInputTarget};
pub mod minimap;
pub(super) mod projection;
pub(super) mod surface;
mod terrain;
mod terrain_updates;
mod tracker;
mod viewport;
mod visibility;
