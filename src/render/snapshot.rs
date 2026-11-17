//! What render-layer drawers read of an object in the presentation frame
//! (docs/sim-thread.md). The app owns the frame (`app::presentation::frame`)
//! and implements these views on it, so the render layer reads a snapshot
//! without depending upward on the app.

use crate::map::entities::EntityCategory;
use crate::sim::intern::InternedId;

/// One object of the presentation frame, as the render drawers read it.
pub(crate) trait SnapshotObject {
    fn stable_id(&self) -> u64;
    fn category(&self) -> EntityCategory;
    fn owner(&self) -> InternedId;
    fn type_ref(&self) -> InternedId;
    fn is_voxel(&self) -> bool;
    fn on_bridge(&self) -> bool;
    fn position(&self) -> &crate::sim::components::Position;
    fn lifecycle(&self) -> &crate::sim::world::ObjectLifecycle;
    fn disguise(&self) -> Option<&crate::sim::cloak_disguise::DisguiseRuntime>;
    /// The cloak's state (`CloakRuntime::state`), 0 without a cloak.
    fn cloak_state(&self) -> i32;
    /// The cloak's depth (`CloakRuntime::depth`), 0 without a cloak.
    fn cloak_depth(&self) -> i32;
    fn veterancy(&self) -> &crate::sim::combat::veterancy::Veterancy;
    fn sinking(&self) -> bool;
    fn building_cloak_stage(&self) -> u8;
    fn native_cloak_offset_words(&self) -> i32;
    fn is_warped_out(&self) -> bool;
    fn is_warping_in(&self) -> bool;
    /// `GameEntity::visual_character`'s screen answer for the local player.
    fn visual_character(&self) -> u8;
    /// `ground_pose::object_center_xy`.
    fn center_xy(&self) -> [i32; 2];
    /// `locomotor_visual::screen_position`.
    fn screen_position(&self) -> (f32, f32);
    /// `ground_pose::object_world_z_leptons` without the map.
    fn world_z_leptons_off_map(&self) -> i32;
    /// What the radar reads of it beyond its record.
    fn radar(&self) -> &crate::render::radar::RadarFacts;
}

/// The presentation frame's objects, by stable id.
pub(crate) trait SnapshotObjects {
    type Object: SnapshotObject;
    fn object(&self, id: u64) -> Option<&Self::Object>;
    fn objects(&self) -> impl Iterator<Item = &Self::Object>;
}
