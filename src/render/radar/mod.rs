//! The sidebar radar (`RadarClass`): the generated terrain surface and its
//! geometry, the retained object tracker, the client-local event array, the
//! housing animation, the minimap renderer that draws them, and VERA's
//! optional HD radar.

pub mod anim;
pub(super) mod compose;
mod current_cell;
mod events;
#[cfg(test)]
pub(crate) use events::saved_radar_fixture;
pub(crate) use events::{ClientRadarEvents, SavedRadarEventsError};
pub(super) mod geometry;
mod hd;
mod helpers;
mod interaction;
pub(crate) use interaction::{NativeRadarCameraTarget, NativeRadarInputTarget};
pub mod minimap;
pub(super) mod projection;
mod terrain;
mod terrain_updates;
pub use terrain_updates::RadarTerrainBacklog;
mod tracker;
mod viewport;
mod visibility;
pub(crate) use visibility::{RadarFacts, RadarFrame};
