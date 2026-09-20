//! Minimap (radar view) — tiny overhead view of the entire map.
//!
//! Shows terrain as colored pixels and entity positions as colored dots
//! in the native 140x108 radar aperture.
//! A side-colored one-pixel rectangle indicates the current camera viewport.
//!
//! ## Implementation
//! - Terrain image is generated at map load and whenever mutable MapClass
//!   LocalSize authority changes.
//! - Unit dots are overlaid on demand by copying the base image and stamping dots.
//! - The combined image is only re-uploaded when sim/fog state changes.
//! - A separate 2x2 white pixel texture is tinted for the viewport rectangle lines.
//!
//! ## Screen-space rendering trick
//! The batch shader subtracts camera_pos from world positions. To render UI elements
//! at fixed screen positions, we add camera_pos back:
//!   `instance.position = screen_pos + camera_offset`
//! This cancels out in the shader: `clip_pos = (screen_pos + cam - cam) / screen_size`.
//!
//! ## Dependency rules
//! - Part of render/ — depends on render/batch, render/gpu, map/terrain, sim/components.
//! - Reads from sim/ via EntityStore iteration (GameEntity.position, .owner) but NEVER mutates sim state.

use crate::map::entities::EntityCategory;
use crate::map::houses::HouseColorMap;
use crate::map::playfield::PlayfieldBounds;
use crate::map::terrain::TerrainGrid;
use crate::render::batch::{BatchRenderer, BatchTexture, SpriteInstance};
use crate::render::gpu::GpuContext;
use crate::rules::house_colors::HouseColorRamps;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::vision::FogState;
use std::collections::{BTreeMap, HashMap};

use super::current_radar_cell::CurrentRadarCellAuthority;
use super::minimap_helpers::{
    COLOR_SHROUD, MINIMAP_DEPTH, MINIMAP_HEIGHT, MINIMAP_WIDTH,
    RadarSurfacePixel, cell_visibility_color, dim_color, set_pixel, surface_visibility_color,
};
use super::radar_tracker::{
    RadarProjectionFacts, RetainedRadarTracker, radar_entity_owner_color,
    radar_pixel_candidate_eligible,
};
use super::radar_visibility::build_radar_object_update;
#[cfg(test)]
use super::radar_tracker::RadarTrackerEntry;
pub use super::minimap_helpers::{OverlayClassification, default_minimap_rect};
pub(crate) use super::minimap_helpers::minimap_overlay_datum;
use super::minimap_helpers::{OverlayPixel, TerrainPixel};
use super::minimap_projection::{
    MinimapPlayfieldProjection, aperture_pixel, generated_primary_copy_frame,
};
#[cfg(test)]
use super::minimap_projection::minimap_screen_point_to_camera_top_left;
use super::native_radar_surface::NativeRadarSurfaceGeometry;
use super::native_radar_terrain::NativeRadarTerrainSurface;
use super::native_radar_viewport::NativeRadarViewportState;
use super::radar_events::{ClientRadarEvents, RadarEventSource};
use super::radar_terrain_updates::{
    RadarTerrainUpdateLayers, stage_radar_terrain_dirty_generation,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MinimapCellRadarSource {
    Overlay {
        overlay_id: u8,
        frame: u8,
        is_tiberium: bool,
        has_cell_anim: bool,
        has_tiberium_type: bool,
    },
    TerrainObject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MinimapOverlayDatum {
    pub rx: u16,
    pub ry: u16,
    pub classification: OverlayClassification,
    pub source: MinimapCellRadarSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlayfieldAuthorityStamp {
    bounds: Option<PlayfieldBounds>,
    revision: u64,
}

fn playfield_authority_needs_reconcile(
    installed: Option<PlayfieldAuthorityStamp>,
    bounds: Option<PlayfieldBounds>,
    revision: u64,
) -> bool {
    installed != Some(PlayfieldAuthorityStamp { bounds, revision })
}

/// Minimap renderer — manages terrain image, unit overlay, and viewport rectangle.
pub struct MinimapRenderer {
    /// Base terrain image in the fixed native 140x108 radar aperture, rebuilt
    /// on a playfield-authority revision.
    base_terrain_rgba: Vec<u8>,
    /// GPU texture containing the current minimap image (terrain + unit dots).
    map_texture: BatchTexture,
    /// Raw GPU texture handle for `write_texture()` reuse (avoids per-frame alloc).
    map_texture_raw: wgpu::Texture,
    /// Reusable RGBA scratch buffer for rebuilding the minimap texture.
    rgba_scratch: Vec<u8>,
    /// Tiny 2x2 white pixel texture for drawing the viewport rectangle lines.
    white_texture: BatchTexture,
    /// Cached world bounds for coordinate mapping.
    pub(super) world_origin_x: f32,
    pub(super) world_origin_y: f32,
    pub(super) world_width: f32,
    pub(super) world_height: f32,
    terrain_pixels: Vec<TerrainPixel>,
    /// Exact generated-primary pixels, including each pixel's native inverse
    /// shroud/fog cell and packed terrain color.
    surface_pixels: Vec<RadarSurfacePixel>,
    /// Per-cell overlay metadata. Authoritative radar surfaces bake these into
    /// raw RGB before sampling; only the mapless fallback stamps them later.
    overlay_pixels: Vec<OverlayPixel>,
    /// Legacy mapless aspect-fit sub-region within the native aperture.
    pub(super) map_offset_x: f32,
    pub(super) map_offset_y: f32,
    pub(super) map_pixel_w: f32,
    pub(super) map_pixel_h: f32,
    /// Exact generated primary-surface geometry used by native radar events.
    pub(super) native_radar_surface: Option<NativeRadarSurfaceGeometry>,
    native_radar_terrain: Option<NativeRadarTerrainSurface>,
    /// BRIDGE1 frame-0 SHP header RGB used by CellClass flag-0x100 cells.
    structural_bridge_radar_color: [u8; 3],
    /// Last simulation tick used to refresh the texture.
    last_sim_tick: u64,
    /// Last fog generation used to refresh the texture.
    last_fog_generation: u64,
    /// Last local owner used for fog-aware refresh.
    last_visibility_owner: Option<InternedId>,
    last_radar_terrain_dirty_generation: u64,
    pub(super) playfield_bounds: Option<PlayfieldBounds>,
    installed_playfield_authority: Option<PlayfieldAuthorityStamp>,
    /// Client-local +0x423/discovery cache; never snapshot/hash authority.
    pub(super) radar_tracker: RetainedRadarTracker,
    /// Client-local type-5 live array and accepted-cell review ring. Native
    /// owns both beside RadarClass surfaces, never in serialized world state.
    radar_events: ClientRadarEvents,
    /// Client-local `RadarClass+0x14DC..0x14F8` current/previous rectangle.
    pub(super) viewport_state: NativeRadarViewportState,
}

impl MinimapRenderer {
    /// Create a new MinimapRenderer, generating the initial terrain image.
    ///
    /// Each terrain cell is mapped to a minimap pixel based on its normalized
    /// position within the world. Color is chosen by TMP radar colors when
    /// available, falling back to tile classification (water/land/elevated).
    /// Overlay data is pre-classified by the caller to avoid render/ depending
    /// on map/overlay_types.
    pub(crate) fn new(
        gpu: &GpuContext,
        batch: &BatchRenderer,
        grid: &TerrainGrid,
        resolved_terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
        overlay_data: &[MinimapOverlayDatum],
        overlay_radar_colors: &HashMap<(u8, u8), [u8; 3]>,
        theater_name: &str,
        playfield_bounds: Option<PlayfieldBounds>,
        playfield_revision: u64,
    ) -> Self {
        let pixel_count: usize = (MINIMAP_WIDTH * MINIMAP_HEIGHT * 4) as usize;
        let projection = MinimapPlayfieldProjection::derive(
            grid,
            resolved_terrain,
            overlay_data,
            overlay_radar_colors,
            theater_name,
            playfield_bounds,
            None,
        );

        let (map_texture_raw, map_texture) =
            batch.create_updatable_texture(
                gpu,
                &projection.base_rgba,
                MINIMAP_WIDTH,
                MINIMAP_HEIGHT,
            );
        let white_texture: BatchTexture = create_white_texture(gpu, batch);
        let rgba_scratch: Vec<u8> = vec![0u8; pixel_count];

        log::info!(
            "Minimap created: {}x{} px, {} terrain cells, {} overlay pixels",
            MINIMAP_WIDTH,
            MINIMAP_HEIGHT,
            projection.terrain_pixels.len(),
            projection.overlay_pixels.len(),
        );

        Self {
            base_terrain_rgba: projection.base_rgba,
            map_texture,
            map_texture_raw,
            rgba_scratch,
            white_texture,
            world_origin_x: projection.world_origin_x,
            world_origin_y: projection.world_origin_y,
            world_width: projection.world_width,
            world_height: projection.world_height,
            terrain_pixels: projection.terrain_pixels,
            surface_pixels: projection.surface_pixels,
            overlay_pixels: projection.overlay_pixels,
            map_offset_x: projection.map_offset_x,
            map_offset_y: projection.map_offset_y,
            map_pixel_w: projection.map_pixel_w,
            map_pixel_h: projection.map_pixel_h,
            native_radar_surface: projection.native_radar_surface,
            native_radar_terrain: projection.native_radar_terrain,
            structural_bridge_radar_color: super::minimap_helpers::structural_bridge_radar_color(
                overlay_radar_colors,
            ),
            last_sim_tick: u64::MAX,
            last_fog_generation: u64::MAX,
            last_visibility_owner: None,
            last_radar_terrain_dirty_generation: u64::MAX,
            playfield_bounds,
            installed_playfield_authority: Some(PlayfieldAuthorityStamp {
                bounds: playfield_bounds,
                revision: playfield_revision,
            }),
            radar_tracker: RetainedRadarTracker::default(),
            radar_events: ClientRadarEvents::default(),
            viewport_state: NativeRadarViewportState::default(),
        }
    }

    /// Synchronously install a changed normalized LocalSize authority and
    /// rebuild the radar surface/mapping. The revision is part of the gate so
    /// a repeated writer still performs native's full RefreshRadar path.
    pub(crate) fn reconcile_playfield(
        &mut self,
        gpu: &GpuContext,
        grid: &TerrainGrid,
        runtime: Option<&crate::sim::runtime::SimRuntime>,
        overlay_data: &[MinimapOverlayDatum],
        overlay_radar_colors: &HashMap<(u8, u8), [u8; 3]>,
        theater_name: &str,
        playfield_bounds: Option<PlayfieldBounds>,
        playfield_revision: u64,
    ) -> bool {
        let authority = PlayfieldAuthorityStamp {
            bounds: playfield_bounds,
            revision: playfield_revision,
        };
        if !playfield_authority_needs_reconcile(
            self.installed_playfield_authority,
            playfield_bounds,
            playfield_revision,
        ) {
            return false;
        }

        let action40_rebuild = self.installed_playfield_authority.is_some();
        let resolved_terrain =
            runtime.and_then(|runtime| runtime.simulation.resolved_terrain.as_ref());
        let current_cell_authority = runtime.map(CurrentRadarCellAuthority::from_runtime);
        let projection = MinimapPlayfieldProjection::derive(
            grid,
            resolved_terrain,
            overlay_data,
            overlay_radar_colors,
            theater_name,
            playfield_bounds,
            current_cell_authority,
        );
        self.base_terrain_rgba = projection.base_rgba;
        self.world_origin_x = projection.world_origin_x;
        self.world_origin_y = projection.world_origin_y;
        self.world_width = projection.world_width;
        self.world_height = projection.world_height;
        self.terrain_pixels = projection.terrain_pixels;
        self.surface_pixels = projection.surface_pixels;
        self.overlay_pixels = projection.overlay_pixels;
        self.map_offset_x = projection.map_offset_x;
        self.map_offset_y = projection.map_offset_y;
        self.map_pixel_w = projection.map_pixel_w;
        self.map_pixel_h = projection.map_pixel_h;
        self.native_radar_surface = projection.native_radar_surface;
        self.native_radar_terrain = projection.native_radar_terrain;
        self.structural_bridge_radar_color =
            super::minimap_helpers::structural_bridge_radar_color(overlay_radar_colors);
        self.playfield_bounds = playfield_bounds;
        self.installed_playfield_authority = Some(authority);
        self.rgba_scratch.resize(self.base_terrain_rgba.len(), 0);
        self.last_sim_tick = u64::MAX;
        self.last_fog_generation = u64::MAX;
        self.last_radar_terrain_dirty_generation = u64::MAX;
        self.viewport_state.reset_for_rebuild();
        if action40_rebuild {
            // FUN_00655990 clears +0x423; FUN_006E21E0 then forces Buildings.
            self.radar_tracker.reset_for_action40();
        } else {
            // A stale/restore reconcile is not itself an action-40 firing.
            self.radar_tracker.reset_for_load_or_view();
            self.radar_events.reset_for_load_or_view();
        }

        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.map_texture_raw,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.base_terrain_rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(MINIMAP_WIDTH * 4),
                rows_per_image: Some(MINIMAP_HEIGHT),
            },
            wgpu::Extent3d {
                width: MINIMAP_WIDTH,
                height: MINIMAP_HEIGHT,
                depth_or_array_layers: 1,
            },
        );
        true
    }

    pub(crate) fn needs_playfield_reconcile(
        &self,
        playfield_bounds: Option<PlayfieldBounds>,
        playfield_revision: u64,
    ) -> bool {
        playfield_authority_needs_reconcile(
            self.installed_playfield_authority,
            playfield_bounds,
            playfield_revision,
        )
    }

    /// Invalidate the dirty-gate so the next update redraws regardless of
    /// counters (F10): after a load the fog view generation restarts and the
    /// restored tick may equal the pre-load tick, so equal counters no longer
    /// prove an unchanged view.
    pub fn mark_stale(&mut self) {
        self.last_sim_tick = u64::MAX;
        self.last_fog_generation = u64::MAX;
        // The restored sim's radar-terrain dirty generation restarts too. The
        // next full reconcile derives from restored live CellClass authority;
        // it never relies on a dirty replay from the abandoned timeline.
        self.last_radar_terrain_dirty_generation = u64::MAX;
        self.installed_playfield_authority = None;
        self.radar_tracker.reset_for_load_or_view();
        self.radar_events.reset_for_load_or_view();
        self.viewport_state.reset_for_rebuild();
    }

    /// Update the minimap texture with unit dot overlays from the ECS world.
    /// Copies terrain, overlays, and entity dots, then re-uploads to the GPU.
    /// Returns a dirty generation only after completion so the app can ack it;
    /// unchanged simulation/view inputs reuse the existing texture.
    pub fn update_unit_dots(
        &mut self,
        gpu: &GpuContext,
        entities: &crate::sim::entity_store::EntityStore,
        logic_order: &[u64],
        houses: &BTreeMap<InternedId, crate::sim::house_state::HouseState>,
        house_colors: &HouseColorMap,
        sim_tick: u64,
        local_owner: Option<InternedId>,
        fog: &FogState,
        full_visibility: bool,
        game_mode_nonzero: bool,
        rules: Option<&RuleSet>,
        interner: Option<&crate::sim::intern::StringInterner>,
        bridge_state: Option<&crate::sim::bridge_state::BridgeRuntimeState>,
        overlay_grid: Option<&crate::sim::overlay_grid::OverlayGrid>,
        overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
        overlay_radar_colors: &HashMap<(u8, u8), [u8; 3]>,
        resolved_terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
        radar_terrain_dirty_cells: &[(u16, u16)],
        radar_terrain_dirty_generation: u64,
    ) -> Option<u64> {
        let fog_generation = if full_visibility { 0 } else { fog.view_generation() };
        let visibility_owner = local_owner;
        if sim_tick == self.last_sim_tick
            && fog_generation == self.last_fog_generation
            && visibility_owner == self.last_visibility_owner
            && radar_terrain_dirty_generation == self.last_radar_terrain_dirty_generation
        {
            return None;
        }
        let staged_radar_terrain_update = stage_radar_terrain_dirty_generation(
            RadarTerrainUpdateLayers {
                base_rgba: &mut self.base_terrain_rgba,
                terrain_pixels: &self.terrain_pixels,
                surface_pixels: &mut self.surface_pixels,
                overlay_pixels: &mut self.overlay_pixels,
                native_surface: self.native_radar_surface,
                native_terrain: &mut self.native_radar_terrain,
            },
            CurrentRadarCellAuthority::new(
                resolved_terrain,
                bridge_state,
                overlay_grid,
                overlay_registry,
                rules,
            ),
            self.structural_bridge_radar_color,
            overlay_radar_colors,
            radar_terrain_dirty_cells,
            radar_terrain_dirty_generation,
            self.last_radar_terrain_dirty_generation,
        );
        if visibility_owner != self.last_visibility_owner && self.last_sim_tick != u64::MAX {
            // g_PlayerPtr controls bucket-front insertion and visibility. A
            // view-owner switch must rebuild, not reuse the other client's
            // ordered tracker.
            self.radar_tracker.reset_for_load_or_view();
            self.radar_events.reset_for_load_or_view();
        }
        let size: u32 = MINIMAP_WIDTH;
        {
            let rgba: &mut Vec<u8> = &mut self.rgba_scratch;

            // Fill scratch buffer: either shroud + fog-aware terrain, or base terrain copy.
            if let Some(local_owner) = local_owner.filter(|_| !full_visibility) {
                for pixel in rgba.chunks_exact_mut(4) {
                    pixel.copy_from_slice(&COLOR_SHROUD);
                }
                let pixels: &[RadarSurfacePixel] = if self.native_radar_terrain.is_some() {
                    &self.surface_pixels
                } else {
                    // The fallback has no authoritative generated surface.
                    // It retains the legacy per-cell presentation path.
                    &[]
                };
                for terrain_pixel in pixels {
                    let color = match surface_visibility_color(local_owner, fog, terrain_pixel) {
                        Some(color) => color,
                        None => continue,
                    };
                    if let Some((x, y)) = aperture_pixel(
                        self.native_radar_surface,
                        (terrain_pixel.px, terrain_pixel.py),
                    ) {
                        set_pixel(rgba, size, x, y, color);
                    }
                }
                if self.native_radar_terrain.is_none() {
                    for terrain_pixel in &self.terrain_pixels {
                        let color = match cell_visibility_color(local_owner, fog, terrain_pixel) {
                            Some(color) => color,
                            None => continue,
                        };
                        if let Some((x, y)) =
                            aperture_pixel(None, (terrain_pixel.px, terrain_pixel.py))
                        {
                            set_pixel(rgba, size, x, y, color);
                        }
                    }
                }
            } else {
                rgba.copy_from_slice(&self.base_terrain_rgba);
            }

            // With authoritative native terrain, CellClass::GetRadarColor has
            // already applied overlay precedence in the raw RGB buffer before
            // weighted sampling. Retain the old stamp only for mapless fallback.
            for overlay in self
                .overlay_pixels
                .iter()
                .filter(|_| self.native_radar_terrain.is_none())
            {
                if let Some(local_owner) = local_owner.filter(|_| !full_visibility) {
                    if !fog.is_cell_revealed(local_owner, overlay.rx, overlay.ry) {
                        continue;
                    }
                    let mut color: [u8; 4] = overlay.color;
                    if overlay.classification == OverlayClassification::Bridge {
                        color = dim_color(color, 0.5);
                    }
                    if let Some((x, y)) =
                        aperture_pixel(self.native_radar_surface, (overlay.px, overlay.py))
                    {
                        set_pixel(rgba, size, x, y, color);
                    }
                } else {
                    let mut color = overlay.color;
                    if overlay.classification == OverlayClassification::Bridge {
                        color = dim_color(color, 0.5);
                    }
                    if let Some((x, y)) =
                        aperture_pixel(self.native_radar_surface, (overlay.px, overlay.py))
                    {
                        set_pixel(rgba, size, x, y, color);
                    }
                }
            }
        }

        // Trigger/action precedes LogicClass: action 40 clears all, forces the
        // reverse Building tail, then mobiles return on ordinary +0x4A0 visits.
        let projection = self.radar_projection_facts();
        self.radar_tracker.remove_absent_or_ineligible(entities);
        if self.radar_tracker.take_action40_building_tail_pending() {
            for stable_id in entities.keys_sorted().into_iter().rev() {
                let Some(entity) = entities.get(stable_id) else {
                    continue;
                };
                if entity.category != EntityCategory::Structure {
                    continue;
                }
                if !entity.lifecycle.object_alive || entity.lifecycle.in_limbo {
                    continue;
                }
                let update = build_radar_object_update(
                    entity,
                    houses,
                    local_owner,
                    fog,
                    full_visibility,
                    game_mode_nonzero,
                    rules,
                    interner,
                    projection,
                    self.playfield_bounds,
                    resolved_terrain,
                );
                if let Some(event) = self.radar_tracker.update_object(update, true) {
                    self.create_enemy_sensed_event(event, sim_tick, rules);
                }
            }
        }
        for &stable_id in logic_order {
            let Some(entity) = entities.get(stable_id) else {
                continue;
            };
            let update = build_radar_object_update(
                entity,
                houses,
                local_owner,
                fog,
                full_visibility,
                game_mode_nonzero,
                rules,
                interner,
                projection,
                self.playfield_bounds,
                resolved_terrain,
            );
            if let Some(event) = self.radar_tracker.update_object(update, false) {
                self.create_enemy_sensed_event(event, sim_tick, rules);
            }
        }
        self.radar_events.finish_baseline();
        let default_event_config = crate::rules::radar_event_config::RadarEventConfig::default();
        let event_config = rules
            .map(|rules| &rules.radar_event_config)
            .unwrap_or(&default_event_config);
        self.radar_events.advance_to_frame(sim_tick, event_config);

        // RenderCellPixel @ 0x00655C50 scans each ordered bucket forward and
        // the first eligible exact-coordinate object supplies the owner color.
        // Resolve the per-house ramp table once (the default empty table only
        // when rules are absent).
        let default_ramps = HouseColorRamps::default();
        let ramps: &HouseColorRamps = rules
            .map(|r| &r.house_color_ramps)
            .unwrap_or(&default_ramps);
        let projection = self.radar_projection_facts();
        let winners = self.radar_tracker.visible_winners(|entry| {
            radar_pixel_candidate_eligible(
                entry,
                entities,
                houses,
                local_owner,
                fog,
                full_visibility,
                game_mode_nonzero,
                rules,
                interner,
                projection,
            )
        });
        let rgba: &mut Vec<u8> = &mut self.rgba_scratch;
        for entry in winners {
            let Some(entity) = entities.get(entry.stable_id) else {
                continue;
            };
            let color = radar_entity_owner_color(entity, interner, house_colors, ramps);
            if entry.x >= 0 && entry.y >= 0 {
                if let Some((x, y)) = aperture_pixel(
                    self.native_radar_surface,
                    (entry.x as u32, entry.y as u32),
                ) {
                    set_pixel(rgba, size, x, y, color);
                }
            }
        }

        // `RadarClass::Update @ 0x00656EC0` composes object pixels, then the
        // ascending radar-event array, then SpySatellite vision. Rust's SpySat
        // materialization is already present in the fog/base inputs; the event
        // outlines must nevertheless remain after object pixels here.
        if let Some(surface) = self.native_radar_surface {
            self.radar_events.draw(
                rgba,
                MINIMAP_WIDTH,
                MINIMAP_HEIGHT,
                surface.generated_size(),
                surface.aperture_offset(),
            );
        }

        let consumed_radar_terrain_generation = staged_radar_terrain_update
            .finish_infallible(&mut self.last_radar_terrain_dirty_generation, || {
                gpu.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.map_texture_raw,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    rgba,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(size * 4),
                        rows_per_image: Some(MINIMAP_HEIGHT),
                    },
                    wgpu::Extent3d {
                        width: size,
                        height: MINIMAP_HEIGHT,
                        depth_or_array_layers: 1,
                    },
                );
            });
        self.last_sim_tick = sim_tick;
        (self.last_fog_generation, self.last_visibility_owner) = (fog_generation, visibility_owner);
        consumed_radar_terrain_generation
    }

    fn radar_projection_facts(&self) -> RadarProjectionFacts {
        RadarProjectionFacts {
            native_surface: self.native_radar_surface,
            world_origin_x: self.world_origin_x,
            world_origin_y: self.world_origin_y,
            world_width: self.world_width,
            world_height: self.world_height,
            map_offset_x: self.map_offset_x,
            map_offset_y: self.map_offset_y,
            map_pixel_w: self.map_pixel_w,
            map_pixel_h: self.map_pixel_h,
        }
    }

    fn create_enemy_sensed_event(
        &mut self,
        event: super::radar_tracker::RadarSensedPresentationEvent,
        sim_tick: u64,
        rules: Option<&RuleSet>,
    ) {
        let Some(surface) = self.native_radar_surface else {
            return;
        };
        // `InitRadarEvent @ 0x0065FB80` consumes CellToRadarPixel's point in
        // the generated primary surface, not the Rust 200x200 texture frame.
        let pixel = surface.cell_to_surface_pixel(event.cell);
        let default_config = crate::rules::radar_event_config::RadarEventConfig::default();
        let config = rules
            .map(|rules| &rules.radar_event_config)
            .unwrap_or(&default_config);
        self.radar_events.create_enemy_sensed(
            RadarEventSource {
                cell: event.cell,
                radar_pixel: pixel,
            },
            sim_tick,
            surface.generated_size(),
            config,
        );
    }

    /// `CreateRadarEvent @ 0x0065FA70` for a simulation-published request the
    /// app already passed through the native caller's own gate. The result
    /// gates that caller's EVA line.
    pub(crate) fn admit_radar_event(
        &mut self,
        request: crate::sim::radar::RadarEventRequest,
        sim_tick: u64,
        rules: Option<&RuleSet>,
    ) -> bool {
        let default_config = crate::rules::radar_event_config::RadarEventConfig::default();
        let config = rules
            .map(|rules| &rules.radar_event_config)
            .unwrap_or(&default_config);
        // `InitRadarEvent @ 0x0065FB80` measures the shrink start from the
        // generated primary surface.
        let geometry = self.native_radar_surface.map(|surface| {
            (
                surface.cell_to_surface_pixel((request.rx, request.ry)),
                surface.generated_size(),
            )
        });
        self.radar_events
            .admit(request, sim_tick, geometry, config)
    }

    /// Spacebar review of the eight most recent accepted event cells.
    pub(crate) fn cycle_radar_event(&mut self, now: std::time::Instant) -> Option<(u16, u16)> {
        self.radar_events.cycle_cell(now)
    }

    /// Build a SpriteInstance that fills the given screen rect with the minimap.
    ///
    /// Native's already-generated primary is copied at its own dimensions and
    /// centered in the aperture; only the no-authority fallback fills the rect.
    pub fn build_minimap_instance_in_rect(
        &self,
        camera_x: f32,
        camera_y: f32,
        screen_x: f32,
        screen_y: f32,
        width: f32,
        height: f32,
    ) -> SpriteInstance {
        let (position, size, uv_origin, uv_size) = self.native_radar_surface.map_or(
            (
                [camera_x + screen_x, camera_y + screen_y],
                [width, height],
                [0.0, 0.0],
                [1.0, 1.0],
            ),
            |surface| {
                let copy = generated_primary_copy_frame(surface, width, height);
                (
                    [
                        camera_x + screen_x + copy.offset[0],
                        camera_y + screen_y + copy.offset[1],
                    ],
                    copy.size,
                    copy.uv_origin,
                    copy.uv_size,
                )
            },
        );
        SpriteInstance {
            position,
            size,
            uv_origin,
            uv_size,
            depth: MINIMAP_DEPTH,
            tint: [1.0, 1.0, 1.0],
            alpha: 1.0,
            ..Default::default()
        }
    }

    /// Already-composed pixel input retained by the ordinary radar housing.
    pub(crate) fn composed_rgba(&self) -> (&[u8], [u32; 2]) {
        (&self.rgba_scratch, [MINIMAP_WIDTH, MINIMAP_HEIGHT])
    }

    /// Get a reference to the minimap texture for drawing.
    pub fn map_texture(&self) -> &BatchTexture {
        &self.map_texture
    }

    /// Get a reference to the white texture for drawing viewport lines.
    pub fn white_texture(&self) -> &BatchTexture {
        &self.white_texture
    }
}

#[cfg(test)]
fn minimap_entity_in_playfield(
    playfield_authority_configured: bool,
    entity: &crate::sim::game_entity::GameEntity,
) -> bool {
    !playfield_authority_configured || entity.in_playfield
}

/// Create a 2x2 solid white texture for drawing lines and rectangles.
fn create_white_texture(gpu: &GpuContext, batch: &BatchRenderer) -> BatchTexture {
    let white_rgba: [u8; 16] = [
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    ];
    batch.create_texture(gpu, &white_rgba, 2, 2)
}

#[cfg(test)]
#[path = "minimap_tests.rs"]
mod tests;
