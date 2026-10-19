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
//! - render/ reads sim state; presentation producers may consume the explicitly
//!   borrowed process Main cursor, never Scenario/MapGen or gameplay state
//! - render/ does NOT depend on: ui/, audio/, net/

pub(crate) mod atlas_growth;
#[cfg(test)]
mod atlas_refresh_retail_tests;
pub mod batch;
pub mod bink_movie;
pub mod bit_font;
pub mod bridge_atlas;
pub mod building_light;
pub mod building_zshape;
pub(crate) mod chrono_screen;
pub mod combat_light;
pub mod cursor_atlas;
#[cfg(test)]
pub(crate) mod depth_gpu_tests;
pub mod draw_state;
pub(crate) mod electric_bolt;
pub(crate) mod foot_depth;
pub mod frame_readback;
pub mod gpu;
pub(crate) mod laser;
pub(crate) mod line_trail;
pub mod loading_screen_chrome;
pub mod locomotor_visual;
pub mod main_menu_shell_chrome;
pub mod native_surface_format;
pub mod native_z;
pub mod overlay_assets;
pub mod overlay_atlas;
pub mod palette_light;
pub mod palette_textures;
pub mod pixel_fx_sparkles;
pub mod radar;
pub(crate) mod radial_indicator;
pub(crate) mod radiation_beam;
pub(crate) mod scaled_copy;
pub mod screenshot;
pub mod selection_overlay;
pub mod shell_paint;
pub mod shell_surface_present;
pub mod shell_text;
pub mod shell_text_reveal;
pub mod shell_transition_pass;
pub mod shroud_buffer;
#[cfg(test)]
mod shroud_gpu_tests;
pub mod sidebar_cameo_atlas;
pub mod sidebar_chrome;
pub mod sidebar_text;
pub(crate) mod sinking;
pub mod skirmish_shell_chrome;
pub mod smudge;
pub mod sprite_atlas;
pub(crate) mod surface_line;
pub mod tactical_compat;
pub mod tactical_draw_plan;
pub(crate) mod tactical_shader;
pub(crate) mod terrain_draw;
#[cfg(test)]
pub(crate) mod terrain_draw_gpu_tests;
pub mod terrain_instances;
pub mod tile_atlas;
pub mod unit_atlas;
pub mod unit_pose_cache;
pub mod upscale_pass;
#[cfg(test)]
mod voxel_parity_tests;
pub mod vxl_normals;
pub mod vxl_raster;
pub mod wave_geometry;

#[cfg(test)]
mod sidebar_gpu_tests;
