//! Minimap (radar view) — tiny overhead view of the entire map.
//!
//! Shows terrain as colored pixels and entity positions as colored dots
//! in the native 140x108 radar aperture.
//! A side-colored one-pixel rectangle indicates the current camera viewport.
//!
//! ## Implementation
//! - Terrain is generated at map load and whenever mutable MapClass
//!   LocalSize authority or a cell's radar colour changes.
//! - Each sim/fog change lists the object, radar-event and beacon pixels in
//!   update order; the GPU composes them over the terrain, shroud and fog
//!   ([`super::compose`]). Only the terrain, the viewer's cell flags and the
//!   pixel list are uploaded, each when it changed.
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
//! - Reads the objects from the presentation frame (`app::presentation::frame`) and
//!   the rest of the world from sim/, but NEVER mutates sim state.

use crate::map::entities::EntityCategory;
use crate::map::houses::HouseColorMap;
use crate::map::playfield::PlayfieldBounds;
use crate::map::terrain::TerrainGrid;
use crate::render::batch::{BatchRenderer, BatchTexture, SpriteInstance};
use crate::render::gpu::GpuContext;
use crate::render::snapshot::SnapshotObject;
use crate::rules::house_colors::HouseColorRamps;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::vision::FogState;
use std::collections::HashMap;

use super::compose::{RadarCompositor, RadarStore, RadarVisibility};
use super::current_cell::CurrentRadarCellAuthority;
use super::events::{ClientRadarEvents, RadarEventSource};
use super::geometry::NativeRadarSurfaceGeometry;
use super::hd::HdRadar;
use super::helpers::{
    COLOR_SHROUD, MINIMAP_DEPTH, MINIMAP_HEIGHT, MINIMAP_WIDTH, cell_visibility_color, dim_color,
    set_pixel,
};
pub use super::helpers::{OverlayClassification, default_minimap_rect};
use super::helpers::{OverlayPixel, TerrainPixels};
pub(crate) use super::helpers::{minimap_overlay_datum, overlay_classification};
#[cfg(test)]
use super::projection::minimap_screen_point_to_camera_top_left;
use super::projection::{MinimapPlayfieldProjection, aperture_pixel, generated_primary_copy_frame};
use super::terrain::NativeRadarTerrainSurface;
use super::terrain_updates::{
    RadarTerrainBacklog, RadarTerrainUpdateLayers, stage_radar_terrain_dirty_generation,
};
#[cfg(test)]
use super::tracker::RadarTrackerEntry;
use super::tracker::{
    RadarProjectionFacts, RetainedRadarTracker, radar_combat_flash_inverts,
    radar_entity_owner_color, radar_pixel_candidate_eligible,
};
use super::viewport::NativeRadarViewportState;
use super::visibility::build_radar_object_update;

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
    /// The mapless fallback's terrain in the fixed native 140x108 radar
    /// aperture, rebuilt on a playfield-authority revision; empty with a
    /// generated surface.
    base_terrain_rgba: Vec<u8>,
    /// The GPU composition and the pictures drawn from it.
    compositor: RadarCompositor,
    /// This update's object, radar-event and beacon pixels, in store order.
    stores: Vec<RadarStore>,
    /// The fog generation, viewer and full-visibility switch whose cell
    /// flags the compositor holds.
    uploaded_visibility: Option<(u64, Option<InternedId>, bool)>,
    /// The mapless fallback's composed terrain and overlays.
    rgba_scratch: Vec<u8>,
    /// Tiny 2x2 white pixel texture for drawing the viewport rectangle lines.
    white_texture: BatchTexture,
    /// Cached world bounds for coordinate mapping.
    pub(super) world_origin_x: f32,
    pub(super) world_origin_y: f32,
    pub(super) world_width: f32,
    pub(super) world_height: f32,
    terrain_pixels: TerrainPixels,
    /// Per-cell overlay metadata. Authoritative radar surfaces bake these into
    /// raw RGB before sampling; only the mapless fallback stamps them later and
    /// updates them for dirty cells.
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
    /// Last relation-colours generation painted, so a toggle while paused still
    /// forces a minimap repaint (see `MatchPresentationState::relation_color_generation`).
    last_relation_color_generation: u64,
    /// The drained simulation radar marks this minimap has not uploaded yet.
    radar_terrain_backlog: RadarTerrainBacklog,
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

impl AsMut<RadarTerrainBacklog> for MinimapRenderer {
    fn as_mut(&mut self) -> &mut RadarTerrainBacklog {
        &mut self.radar_terrain_backlog
    }
}

impl MinimapRenderer {
    /// Create a new MinimapRenderer, generating the initial terrain image.
    ///
    /// Native scenario687AA0/post-load685162 -> Rebuild654650 -> Fill654EA0
    /// queries live Cell47C060 at654F99. Use the same current authority as
    /// dirty updates and reconciliation; authored rows are only the explicit
    /// fallback when no simulation runtime is installed.
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        grid: &TerrainGrid,
        runtime: Option<&crate::sim::runtime::SimRuntime>,
        overlay_data: &[MinimapOverlayDatum],
        overlay_radar_colors: &HashMap<(u8, u8), [u8; 3]>,
        theater_name: &str,
        playfield_bounds: Option<PlayfieldBounds>,
        playfield_revision: u64,
    ) -> Self {
        let resolved_terrain = runtime.and_then(|runtime| runtime.view().resolved_terrain());
        let projection = MinimapPlayfieldProjection::derive(
            grid,
            resolved_terrain,
            overlay_data,
            overlay_radar_colors,
            theater_name,
            playfield_bounds,
            runtime.map(CurrentRadarCellAuthority::from_runtime),
        );

        let mut compositor = RadarCompositor::new(device, queue, batch);
        install_base_picture(
            &mut compositor,
            device,
            queue,
            projection.native_radar_terrain.as_ref(),
            &projection.base_rgba,
        );
        let white_texture: BatchTexture = create_white_texture(device, queue, batch);
        let rgba_scratch: Vec<u8> = vec![0u8; projection.base_rgba.len()];

        log::info!(
            "Minimap created: {}x{} px, {} terrain cells, {} overlay pixels",
            MINIMAP_WIDTH,
            MINIMAP_HEIGHT,
            projection.terrain_pixels.len(),
            projection.overlay_pixels.len(),
        );

        Self {
            base_terrain_rgba: projection.base_rgba,
            compositor,
            stores: Vec::new(),
            uploaded_visibility: None,
            rgba_scratch,
            white_texture,
            world_origin_x: projection.world_origin_x,
            world_origin_y: projection.world_origin_y,
            world_width: projection.world_width,
            world_height: projection.world_height,
            terrain_pixels: projection.terrain_pixels,
            overlay_pixels: projection.overlay_pixels,
            map_offset_x: projection.map_offset_x,
            map_offset_y: projection.map_offset_y,
            map_pixel_w: projection.map_pixel_w,
            map_pixel_h: projection.map_pixel_h,
            native_radar_surface: projection.native_radar_surface,
            native_radar_terrain: projection.native_radar_terrain,
            structural_bridge_radar_color: super::helpers::structural_bridge_radar_color(
                overlay_radar_colors,
            ),
            last_sim_tick: u64::MAX,
            last_fog_generation: u64::MAX,
            last_visibility_owner: None,
            last_radar_terrain_dirty_generation: u64::MAX,
            last_relation_color_generation: u64::MAX,
            radar_terrain_backlog: RadarTerrainBacklog::default(),
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

    /// VERA's HD radar: draw the radar at `scale` times its native pixels, or
    /// natively for none or 1. Only a generated surface has an HD form; the
    /// mapless fallback stays native.
    pub(crate) fn set_hd_scale(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        scale: Option<u32>,
    ) {
        let scale = scale
            .filter(|&scale| scale > 1)
            .filter(|_| self.native_radar_terrain.is_some());
        if self.hd_scale() == scale {
            return;
        }
        let Some(terrain) = &mut self.native_radar_terrain else {
            return;
        };
        terrain.set_hd_scale(scale);
        // The next encode draws the new picture from the retained stores.
        self.compositor
            .set_hd(device, queue, batch, scale.map(|scale| (scale, &*terrain)));
    }

    /// The HD radar's scale while it is drawn.
    pub(super) fn hd_scale(&self) -> Option<u32> {
        self.compositor.hd().map(HdRadar::scale)
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
        let resolved_terrain = runtime.and_then(|runtime| runtime.simulation.resolved_terrain());
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
        self.overlay_pixels = projection.overlay_pixels;
        self.map_offset_x = projection.map_offset_x;
        self.map_offset_y = projection.map_offset_y;
        self.map_pixel_w = projection.map_pixel_w;
        self.map_pixel_h = projection.map_pixel_h;
        self.native_radar_surface = projection.native_radar_surface;
        self.native_radar_terrain = projection.native_radar_terrain;
        // The next `set_hd_scale` builds the HD radar for the new surface.
        install_base_picture(
            &mut self.compositor,
            &gpu.device,
            &gpu.queue,
            self.native_radar_terrain.as_ref(),
            &self.base_terrain_rgba,
        );
        self.uploaded_visibility = None;
        self.structural_bridge_radar_color =
            super::helpers::structural_bridge_radar_color(overlay_radar_colors);
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
            self.radar_events.restart_tracker_baseline();
        }
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
    /// Queue radar terrain cells drained from the simulation for the next
    /// update (see [`RadarTerrainBacklog`]).
    pub fn queue_radar_terrain_cells(&mut self, cells: impl IntoIterator<Item = (u16, u16)>) {
        self.radar_terrain_backlog.extend(cells);
    }

    pub fn mark_stale(&mut self) {
        self.last_sim_tick = u64::MAX;
        self.last_fog_generation = u64::MAX;
        // The next full reconcile derives from restored live CellClass
        // authority; it never relies on a dirty replay from the abandoned
        // timeline, so that timeline's undrawn marks go too.
        self.last_radar_terrain_dirty_generation = u64::MAX;
        self.radar_terrain_backlog.clear();
        self.uploaded_visibility = None;
        self.installed_playfield_authority = None;
        self.radar_tracker.reset_for_load_or_view();
        self.radar_events.restart_tracker_baseline();
        self.viewport_state.reset_for_rebuild();
    }

    pub(crate) fn saved_radar_events(
        &self,
    ) -> crate::sim::snapshot::saved_local_view::SavedRadarEvents {
        self.radar_events.saved()
    }

    /// Successful Load660840/PostLoad660C50 replaces the live alert array,
    /// retaining animation while the already-prepared owner has an empty
    /// review ring and a fresh sighting baseline.
    pub(crate) fn restore_radar_events(&mut self, events: ClientRadarEvents) {
        self.radar_events = events;
    }

    /// Update the radar from the ECS world: the terrain's dirty cells, the
    /// viewer's shroud and fog, then the object, radar-event and beacon
    /// pixels, uploaded for [`Self::encode_composition`] to compose.
    /// Returns a dirty generation only after completion so the app can ack it;
    /// unchanged simulation/view inputs reuse the existing picture.
    pub fn update_unit_dots(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &impl crate::render::snapshot::SnapshotObjects,
        logic_order: &[u64],
        houses: &crate::sim::houses::Houses,
        house_colors: &HouseColorMap,
        sim_tick: u64,
        binary_frame: u32,
        local_owner: Option<InternedId>,
        fog: &FogState,
        alliances: &crate::sim::house_diplomacy::HouseAlliances,
        full_visibility: bool,
        game_mode_nonzero: bool,
        rules: Option<&RuleSet>,
        interner: Option<&crate::sim::intern::StringInterner>,
        overlay_grid: Option<&crate::sim::overlay_grid::OverlayGrid>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        overlay_radar_colors: &HashMap<(u8, u8), [u8; 3]>,
        resolved_terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
        beacons: Option<(
            &crate::render::beacon_art::BeaconArt,
            &[crate::render::beacon_art::BeaconDraw],
        )>,
        relation_color_generation: u64,
    ) -> Option<u64> {
        let fog_generation = if full_visibility {
            0
        } else {
            fog.view_generation()
        };
        let visibility_owner = local_owner;
        if sim_tick == self.last_sim_tick
            && fog_generation == self.last_fog_generation
            && visibility_owner == self.last_visibility_owner
            && self.radar_terrain_backlog.generation() == self.last_radar_terrain_dirty_generation
            && relation_color_generation == self.last_relation_color_generation
        {
            return None;
        }
        let staged_radar_terrain_update = stage_radar_terrain_dirty_generation(
            RadarTerrainUpdateLayers {
                base_rgba: &mut self.base_terrain_rgba,
                terrain_pixels: &self.terrain_pixels,
                overlay_pixels: &mut self.overlay_pixels,
                native_surface: self.native_radar_surface,
                native_terrain: &mut self.native_radar_terrain,
            },
            CurrentRadarCellAuthority::new(resolved_terrain, overlay_grid, overlay_registry, rules),
            self.structural_bridge_radar_color,
            overlay_radar_colors,
            self.radar_terrain_backlog.cells(),
            self.radar_terrain_backlog.generation(),
            self.last_radar_terrain_dirty_generation,
        );
        if visibility_owner != self.last_visibility_owner && self.last_sim_tick != u64::MAX {
            // g_PlayerPtr controls bucket-front insertion and visibility. A
            // view-owner switch must rebuild, not reuse the other client's
            // ordered tracker.
            self.radar_tracker.reset_for_load_or_view();
            self.radar_events.reset_for_view_change();
        }
        // A generated surface's terrain, shroud and fog are composed on the
        // GPU; the mapless fallback composes its own here.
        if self.native_radar_terrain.is_none() {
            self.compose_fallback_terrain(local_owner.filter(|_| !full_visibility), fog);
        }

        // Trigger/action precedes LogicClass: action 40 clears all, forces the
        // reverse Building tail, then mobiles return on ordinary +0x4A0 visits.
        let projection = self.radar_projection_facts();
        // One isolated query per pass: radar lookups stamp their own copy
        // of the fallback Cell, never the simulation's.
        let cells = resolved_terrain.map(crate::map::resolved_terrain::NativeCellQuery::isolated);
        self.radar_tracker.remove_absent_or_ineligible(frame);
        if self.radar_tracker.take_action40_building_tail_pending() {
            // The frame's records are in stable-id order.
            let records: Vec<_> = frame.objects().collect();
            for entity in records.into_iter().rev() {
                if entity.category() != EntityCategory::Structure {
                    continue;
                }
                if !entity.lifecycle().object_alive() || entity.lifecycle().in_limbo() {
                    continue;
                }
                let update = build_radar_object_update(
                    entity,
                    houses,
                    local_owner,
                    fog,
                    alliances,
                    full_visibility,
                    game_mode_nonzero,
                    rules,
                    interner,
                    projection,
                    self.playfield_bounds,
                    cells.as_ref(),
                );
                if let Some(event) = self.radar_tracker.update_object(update, true) {
                    self.create_enemy_sensed_event(event, sim_tick, rules);
                }
            }
        }
        for &stable_id in logic_order {
            let Some(entity) = frame.object(stable_id) else {
                continue;
            };
            let update = build_radar_object_update(
                entity,
                houses,
                local_owner,
                fog,
                alliances,
                full_visibility,
                game_mode_nonzero,
                rules,
                interner,
                projection,
                self.playfield_bounds,
                cells.as_ref(),
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
        let terrain = self.native_radar_terrain.as_ref();
        let winners = self.radar_tracker.visible_winners(|entry| {
            let pixel_cell = match terrain {
                Some(terrain) => Some(terrain.visibility_cell((entry.x, entry.y))),
                None => projection.mapless_pixel_to_cell(entry.x, entry.y),
            };
            radar_pixel_candidate_eligible(
                entry,
                frame,
                houses,
                local_owner,
                fog,
                alliances,
                full_visibility,
                game_mode_nonzero,
                rules,
                interner,
                pixel_cell,
                cells.as_ref(),
            )
        });
        self.stores.clear();
        // An undisguised object's pixel takes its owner's colour, the same
        // for every pixel of that house in this update.
        let mut owner_colors: Vec<(InternedId, [u8; 4])> = Vec::new();
        for entry in winners {
            let Some(entity) = frame.object(entry.stable_id) else {
                continue;
            };
            let disguised = entity.disguise().is_some_and(|d| d.is_disguised());
            let color = match owner_colors
                .iter()
                .find(|(owner, _)| !disguised && *owner == entity.owner())
            {
                Some(&(_, color)) => color,
                None => {
                    // Radar655F48 asks the disguise's house, and with it
                    // whether the owner allies the observer, only of a
                    // disguised object.
                    let owner_allied_with_observer = disguised
                        && local_owner.is_some_and(|observer| {
                            entity.owner() == observer
                                || interner.is_some_and(|interner| {
                                    alliances.is_allied(interner, entity.owner(), observer)
                                })
                        });
                    let color = radar_entity_owner_color(
                        entity,
                        interner,
                        house_colors,
                        ramps,
                        owner_allied_with_observer,
                    );
                    if !disguised {
                        owner_colors.push((entity.owner(), color));
                    }
                    color
                }
            };
            let color = if radar_combat_flash_inverts(entity, local_owner, rules) {
                crate::render::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION
                    .invert_rgba8(color)
            } else {
                color
            };
            if entry.x >= 0
                && entry.y >= 0
                && let Some(pixel) =
                    aperture_pixel(self.native_radar_surface, (entry.x as u32, entry.y as u32))
            {
                self.stores.extend(RadarStore::at(pixel, color));
            }
        }

        // `RadarClass::Update @ 0x00656EC0` composes object pixels, then the
        // ascending radar-event array, then the beacon loop (`0x00431700` →
        // `0x00430650` at `0x0065756E`; Ghidra's "SpySatellite" names are
        // wrong: the receiver is the beacon manager `0x0089C3B0`, gated on the
        // RDRBEACN counts). The event outlines must remain after object pixels.
        if let Some(surface) = self.native_radar_surface {
            self.radar_events.draw(
                (MINIMAP_WIDTH, MINIMAP_HEIGHT),
                surface.generated_size(),
                surface.aperture_offset(),
                |pixel, color| self.stores.extend(RadarStore::at(pixel, color)),
            );
            // Then the beacons (`0x0065756E -> 0x00431700 -> 0x00430650`),
            // clipped to the generated surface.
            for (art, beacon) in beacons
                .into_iter()
                .flat_map(|(art, records)| records.iter().map(move |beacon| (art, beacon)))
            {
                let center = surface.world_to_surface_pixel(beacon.coord[0], beacon.coord[1], true);
                art.blit_radar(beacon.house, binary_frame, center, |(x, y), color| {
                    if let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y))
                        && let Some(pixel) = aperture_pixel(Some(surface), (x, y))
                    {
                        self.stores.extend(RadarStore::at(pixel, color));
                    }
                });
            }
        }

        let visibility = (fog_generation, visibility_owner, full_visibility);
        let consumed_radar_terrain_generation = staged_radar_terrain_update.finish_infallible(
            &mut self.last_radar_terrain_dirty_generation,
            || {
                if let Some(terrain) = &self.native_radar_terrain {
                    self.compositor.refresh_terrain(queue, terrain);
                    if self.uploaded_visibility != Some(visibility) {
                        // The plane's flags, gathered from its shared chunks.
                        let plane = local_owner.filter(|_| !full_visibility).map(|owner| {
                            fog.by_owner()
                                .get(&owner)
                                .map(|plane| (plane.cells_raw(), (plane.width(), plane.height())))
                        });
                        let shown = match &plane {
                            None => RadarVisibility::Revealed,
                            Some(None) => RadarVisibility::Shrouded,
                            Some(Some((flags, size))) => {
                                RadarVisibility::Plane { flags, size: *size }
                            }
                        };
                        self.compositor.set_visibility(device, queue, shown);
                        self.uploaded_visibility = Some(visibility);
                    }
                } else {
                    self.compositor.set_fallback_base(queue, &self.rgba_scratch);
                }
                self.compositor.set_stores(device, queue, &self.stores);
            },
        );
        self.last_sim_tick = sim_tick;
        (self.last_fog_generation, self.last_visibility_owner) = (fog_generation, visibility_owner);
        self.last_relation_color_generation = relation_color_generation;
        consumed_radar_terrain_generation
    }

    /// The mapless fallback's terrain and overlays, under `viewer`'s shroud
    /// and fog (every cell revealed without one), into `rgba_scratch`.
    fn compose_fallback_terrain(&mut self, viewer: Option<InternedId>, fog: &FogState) {
        let rgba = &mut self.rgba_scratch;
        let size = MINIMAP_WIDTH;
        if let Some(viewer) = viewer {
            for pixel in rgba.as_chunks_mut::<4>().0 {
                pixel.copy_from_slice(&COLOR_SHROUD);
            }
            for terrain_pixel in self.terrain_pixels.iter() {
                if let Some(color) = cell_visibility_color(viewer, fog, terrain_pixel)
                    && let Some((x, y)) = aperture_pixel(None, (terrain_pixel.px, terrain_pixel.py))
                {
                    set_pixel(rgba, size, x, y, color);
                }
            }
        } else {
            rgba.copy_from_slice(&self.base_terrain_rgba);
        }
        for overlay in &self.overlay_pixels {
            if viewer.is_some_and(|viewer| !fog.is_cell_revealed(viewer, overlay.rx, overlay.ry)) {
                continue;
            }
            let color = if overlay.classification == OverlayClassification::Bridge {
                dim_color(overlay.color, 0.5)
            } else {
                overlay.color
            };
            if let Some((x, y)) =
                aperture_pixel(self.native_radar_surface, (overlay.px, overlay.py))
            {
                set_pixel(rgba, size, x, y, color);
            }
        }
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
        event: super::tracker::RadarSensedPresentationEvent,
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
        self.radar_events.admit(request, sim_tick, geometry, config)
    }

    /// The allied successful reinforcement caller65DB97 only remembers its
    /// requested origin. Native660B80 shares the existing radar review ring;
    /// it does not admit a live radar event or request any announcement.
    pub(crate) fn remember_event_cell(&mut self, cell: (i16, i16)) {
        self.radar_events.remember_event_cell(cell);
    }

    /// Spacebar review of the eight most recent accepted event cells.
    pub(crate) fn cycle_radar_event(
        &mut self,
        now: crate::util::time::Instant,
    ) -> Option<(u16, u16)> {
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

    /// Compose the pictures an update asked for: encode it before the
    /// sidebar draws [`Self::map_texture`] and the housing keeps
    /// [`Self::native_map_texture`].
    pub fn encode_composition(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        self.compositor.encode(device, encoder);
    }

    /// A number that changes whenever [`Self::map_texture`]'s picture does.
    pub fn picture_generation(&self) -> u64 {
        self.compositor.generation()
    }

    /// The minimap picture for drawing: the HD radar's while it is on. Both
    /// fill the same normalized copy frame.
    pub fn map_texture(&self) -> &BatchTexture {
        self.compositor
            .hd()
            .map_or(self.compositor.picture(), HdRadar::picture)
    }

    /// The native 140x108 picture, which the radar housing retains.
    pub fn native_map_texture(&self) -> &BatchTexture {
        self.compositor.picture()
    }

    /// Get a reference to the white texture for drawing viewport lines.
    pub fn white_texture(&self) -> &BatchTexture {
        &self.white_texture
    }
}

/// Give `compositor` the generated surface, or the mapless fallback's
/// `base` terrain, and nothing over it yet.
fn install_base_picture(
    compositor: &mut RadarCompositor,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    terrain: Option<&NativeRadarTerrainSurface>,
    base: &[u8],
) {
    compositor.set_surface(device, queue, terrain);
    if terrain.is_none() {
        compositor.set_fallback_base(queue, base);
    }
    compositor.set_visibility(device, queue, RadarVisibility::Revealed);
    compositor.set_stores(device, queue, &[]);
}

#[cfg(test)]
fn minimap_entity_in_playfield(
    playfield_authority_configured: bool,
    entity: &crate::sim::game_entity::GameEntity,
) -> bool {
    !playfield_authority_configured || entity.in_playfield()
}

/// Create a 2x2 solid white texture for drawing lines and rectangles.
fn create_white_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    batch: &BatchRenderer,
) -> BatchTexture {
    let white_rgba: [u8; 16] = [
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    ];
    batch.create_texture_on_device(device, queue, &white_rgba, 2, 2, None)
}

#[cfg(test)]
#[path = "minimap_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "minimap_gpu_tests.rs"]
mod gpu_tests;
