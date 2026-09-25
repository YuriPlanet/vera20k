//! GPU rendering subsystem using wgpu (WebGPU API).
//!
//! Handles all visual output: terrain tiles, sprites, voxel models, UI overlays.
//! Uses wgpu for cross-platform GPU access (Vulkan, DX12, Metal backends).
//!
//! ## Architecture
//! - `gpu.rs` — wgpu device/queue/surface initialization and frame management
//! - `batch.rs` — instanced sprite batch renderer (textured quads via GPU instancing)
//! - `sprite_atlas.rs` — SHP sprite atlas builder (infantry, buildings)
//! - `unit_atlas.rs` — VXL unit atlas builder (vehicles, voxel models)
//! - `vxl_raster.rs` — software voxel rasterizer (renders .vxl to sprite textures)
//!
//! ## Dependency rules
//! - render/ may READ from: assets/, map/, sim/
//! - render/ NEVER mutates sim state — strictly read-only access
//! - render/ does NOT depend on: ui/, sidebar/, audio/, net/

pub(crate) mod atlas_growth;
#[cfg(test)]
mod atlas_refresh_retail_tests;
pub mod batch;
pub mod bink_movie;
pub mod bit_font;
pub mod bridge_atlas;
pub mod bridge_railing_atlas;
pub mod building_light;
pub mod building_zshape;
pub mod combat_light;
pub mod cursor_atlas;
pub mod draw_state;
pub mod egui_integration;
pub mod frame_readback;
pub(crate) mod foot_depth;
pub mod gpu;
pub mod loading_screen_chrome;
pub mod locomotor_visual;
pub mod main_menu_shell_chrome;
pub mod minimap;
mod current_radar_cell;
mod minimap_interaction;
mod minimap_helpers;
mod minimap_projection;
mod native_radar_surface;
mod native_radar_terrain;
mod native_radar_viewport;
mod radar_events;
mod radar_terrain_updates;
mod radar_tracker;
mod radar_visibility;
pub mod native_surface_format;
pub mod native_z;
pub(crate) mod tactical_shader;
pub(crate) mod terrain_draw;
#[cfg(test)]
pub(crate) mod terrain_draw_gpu_tests;
pub mod overlay_assets;
pub mod terrain_instances;
pub mod overlay_atlas;
pub mod palette_light;
pub mod palette_textures;
pub mod pixel_fx_sparkles;
pub mod radar_anim;
mod radar_animation;
mod radar_surface;
pub mod screenshot;
pub mod selection_overlay;
pub mod shell_paint;
pub mod shell_surface_present;
pub mod shell_text;
pub mod shell_text_reveal;
pub mod shell_transition_pass;
pub mod shroud_buffer;
pub mod sidebar_cameo_atlas;
pub mod sidebar_chrome;
pub mod sidebar_text;
pub mod skirmish_shell_chrome;
pub mod smudge;
pub mod sprite_atlas;
pub mod tactical_compat;
pub mod tactical_draw_plan;
pub mod tile_atlas;
pub mod unit_atlas;
pub mod unit_pose_cache;
pub mod unit_slope_transition_cache;
pub mod upscale_pass;
#[cfg(test)]
mod voxel_parity_tests;
pub mod vxl_normals;
pub mod vxl_raster;
pub mod wave_geometry;
#[cfg(test)]
pub(crate) mod depth_gpu_tests;

#[cfg(test)]
mod sidebar_gpu_tests;
