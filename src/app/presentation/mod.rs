//! Presentation owner (F12): the in-game render pipeline, per-entity instance
//! builders, sidebar rendering, building/fire/light/chute animation runtimes,
//! UI overlays, selection brackets, target lines, spawn pick, and the overlay
//! render index.

pub(crate) mod building_anim;
pub(crate) mod combat_lights;
pub(crate) mod fire_effects;
pub(crate) mod instances;
pub(crate) mod overlay_index;
pub(crate) mod radiation_light;
pub(crate) mod render;
pub(crate) mod selection_brackets;
pub(crate) mod state;
pub(crate) mod sidebar_build;
pub(crate) mod sidebar_gadgets;
pub(crate) mod sidebar_render;
pub(crate) mod sidebar_text;
pub(crate) mod spawn_pick;
pub(crate) mod target_lines;
pub(crate) mod ui_overlays;

pub(crate) mod lighting;

#[cfg(test)]
mod lighting_gpu_tests;
