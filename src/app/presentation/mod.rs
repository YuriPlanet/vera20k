//! Presentation owner (F12): the in-game render pipeline, per-entity instance
//! builders, sidebar rendering, building/fire/light/chute animation runtimes,
//! UI overlays, selection brackets, target lines, and the overlay render index.

pub(crate) mod airstrike_lines;
pub(crate) mod ares_tactical_text;
pub(crate) mod beacon_labels;
pub(crate) mod build_text;
pub(crate) mod building_anim;
pub(crate) mod combat_lights;
pub(crate) mod control_links;
pub(crate) mod detail;
#[cfg(feature = "phobos")]
pub(crate) mod digital_display;
pub(crate) mod display_members;
pub(crate) mod electric_bolts;
pub(crate) mod field_parts;
pub(crate) mod layout_parts;
pub(crate) mod player_layout;
pub(crate) mod fire_effects;
pub(crate) mod flying_credits;
pub(crate) mod frame;
pub(crate) mod frame_scene;
pub(crate) mod house_ui;
pub(crate) mod instances;
pub(crate) mod ion_blasts;
pub(crate) mod lasers;
pub(crate) mod line_trails;
pub(crate) mod observer_strip;
pub(crate) mod outcome_banner;
pub(crate) mod overlay_index;
#[cfg(feature = "phobos")]
pub(crate) mod placement_preview;
pub(crate) mod radial_indicators;
pub(crate) mod radiation_beams;
pub(crate) mod render;
pub(crate) mod render_pose;
pub(crate) mod screen_shake;
pub(crate) mod selection_brackets;
pub(crate) mod sidebar_build;
pub(crate) mod sidebar_gadgets;
pub(crate) mod sidebar_render;
pub(crate) mod sidebar_surface;
pub(crate) mod sidebar_text;
pub(crate) mod state;
#[cfg(feature = "phobos")]
pub(crate) mod sw_sidebar;
#[cfg(feature = "phobos")]
pub(crate) mod sw_sidebar_build;
pub(crate) mod super_timer_groups;
pub(crate) mod super_timers;
pub(crate) mod tactical_composite;
pub(crate) mod talk_bubble;
pub(crate) mod target_lines;
pub(crate) mod ui_overlays;
pub(crate) mod window_ui;

#[cfg(all(test, any(tests_app_presentation, tests_app_presentation_render)))]
mod passenger_pip_tests;

#[cfg(all(test, tests_app_presentation))]
mod ammo_pip_tests;

#[cfg(all(test, tests_app_presentation))]
mod spawn_pip_tests;

#[cfg(all(test, tests_app_presentation))]
mod absorber_pip_tests;

pub(crate) mod lighting;

#[cfg(all(test, tests_app_presentation))]
mod lighting_gpu_tests;
