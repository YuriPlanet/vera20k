//! Presentation owner (F12): the in-game render pipeline, per-entity instance
//! builders, sidebar rendering, building/fire/light/chute animation runtimes,
//! UI overlays, selection brackets, target lines, and the overlay render index.

pub(crate) mod airstrike_lines;
pub(crate) mod building_anim;
pub(crate) mod combat_lights;
pub(crate) mod control_links;
pub(crate) mod detail;
pub(crate) mod electric_bolts;
pub(crate) mod fire_effects;
pub(crate) mod house_ui;
pub(crate) mod instances;
pub(crate) mod interpolation;
pub(crate) mod ion_blasts;
pub(crate) mod lasers;
pub(crate) mod line_trails;
pub(crate) mod outcome_banner;
pub(crate) mod overlay_index;
pub(crate) mod radial_indicators;
pub(crate) mod radiation_beams;
pub(crate) mod render;
pub(crate) mod selection_brackets;
pub(crate) mod sidebar_build;
pub(crate) mod sidebar_gadgets;
pub(crate) mod sidebar_render;
pub(crate) mod sidebar_surface;
pub(crate) mod sidebar_text;
pub(crate) mod state;
pub(crate) mod super_timers;
pub(crate) mod talk_bubble;
pub(crate) mod target_lines;
pub(crate) mod ui_overlays;

#[cfg(test)]
mod passenger_pip_tests;

#[cfg(test)]
mod ammo_pip_tests;

#[cfg(test)]
mod spawn_pip_tests;

#[cfg(test)]
mod absorber_pip_tests;

pub(crate) mod lighting;

#[cfg(test)]
mod lighting_gpu_tests;
