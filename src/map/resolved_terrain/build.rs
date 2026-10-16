//! Map construction (Fill): materializing map cells into resolved terrain,
//! TMP tile metadata, overlay classification and the height maps.

use super::bridge_flags::apply_native_bridge_flag_stamp_to_parts;
use super::native_cell::native_resolved_cell_index;
use super::tubes::{build_auto_low_bridge_tubes, seed_explicit_map_tubes};
use super::*;

/// Identifies whether map overlays still require the ordinary authored-map
/// `OverlayClass::Mark` pass. RMG writes its complete overlay/data rectangles
/// directly into live cells; serializing those cells through `MapFile` must not
/// turn loading into a second Mark operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayLoadSource {
    Authored,
    GeneratedMaterialized,
}

/// Controls whether Fill stops at pristine CellClass materialization or also
/// performs the legacy eager projection retained for synthetic/generated
/// callers. Active authored loads execute LAT, OverlayClass::Mark, Recalc,
/// Terrain occupation, tile Anim construction, CliffBack, and Tube creation at
/// their native later seams, so none of those effects belong in Fill itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TerrainBuildProjection {
    EagerCompatibility,
    PendingAuthored,
}

impl TerrainBuildProjection {
    const fn is_eager(self) -> bool {
        matches!(self, Self::EagerCompatibility)
    }
}

/// Type barrier for the pristine authored Fill product. It intentionally does
/// not expose `ResolvedTerrainGrid`'s pathing/presentation surface: the inner
/// grid must first be consumed by the native Tube/overlay/Recalc transaction.
pub struct AuthoredTerrainFill(ResolvedTerrainGrid);

impl AuthoredTerrainFill {
    pub(crate) fn into_pending_grid(self) -> ResolvedTerrainGrid {
        self.0
    }

    pub fn dimensions(&self) -> (u16, u16) {
        (self.0.width(), self.0.height())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn pending_grid(&self) -> &ResolvedTerrainGrid {
        &self.0
    }
}

impl ResolvedTerrainGrid {
    pub fn build(
        map: &MapFile,
        theater_data: Option<&TheaterData>,
        asset_manager: Option<&crate::assets::asset_manager::AssetManager>,
        terrain_rules: Option<&TerrainRules>,
        overlay_registry: Option<&OverlayTypeRegistry>,
        lat_enabled: bool,
        cliff_back_impassability: u8,
    ) -> Self {
        Self::build_inner(
            map,
            theater_data,
            asset_manager,
            terrain_rules,
            overlay_registry,
            None,
            lat_enabled,
            cliff_back_impassability,
            OverlayLoadSource::Authored,
            TerrainBuildProjection::EagerCompatibility,
            None,
            None,
            None,
            PixelConversionBounds::default(),
        )
    }

    /// Selector-free generated-map load seam for focused construction tests.
    /// Production generated maps use `build_with_variant_selector_and_shared_dummy`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn build_generated_materialized_for_test(map: &MapFile) -> Self {
        Self::build_inner(
            map,
            None,
            None,
            None,
            None,
            None,
            false,
            0,
            OverlayLoadSource::GeneratedMaterialized,
            TerrainBuildProjection::EagerCompatibility,
            None,
            None,
            None,
            PixelConversionBounds::default(),
        )
    }

    /// Production map-load path with the process-owned native variant selector.
    pub fn build_with_variant_selector(
        map: &MapFile,
        theater_data: Option<&TheaterData>,
        asset_manager: Option<&crate::assets::asset_manager::AssetManager>,
        terrain_rules: Option<&TerrainRules>,
        overlay_registry: Option<&OverlayTypeRegistry>,
        terrain_object_types: Option<&HashMap<String, TerrainObjectType>>,
        lat_enabled: bool,
        cliff_back_impassability: u8,
        scenario_fill_ranged: &mut dyn FnMut(u32, u32) -> u32,
        variant_selector: &mut TileVariantSelectionContext<'_, '_>,
    ) -> Self {
        Self::build_inner(
            map,
            theater_data,
            asset_manager,
            terrain_rules,
            overlay_registry,
            terrain_object_types,
            lat_enabled,
            cliff_back_impassability,
            OverlayLoadSource::Authored,
            TerrainBuildProjection::EagerCompatibility,
            Some(scenario_fill_ranged),
            Some(variant_selector),
            None,
            PixelConversionBounds::default(),
        )
    }

    /// Production map-load path whose CellClass grid is born with the process
    /// dummy already attached. This is required before the post-Resize
    /// OverlayPack `SetBridgeDirection_*` pass; binding afterward loses every
    /// missing-neighbor write to the shared fallback object.
    pub fn build_with_variant_selector_and_shared_dummy(
        map: &MapFile,
        theater_data: Option<&TheaterData>,
        asset_manager: Option<&crate::assets::asset_manager::AssetManager>,
        terrain_rules: Option<&TerrainRules>,
        overlay_registry: Option<&OverlayTypeRegistry>,
        terrain_object_types: Option<&HashMap<String, TerrainObjectType>>,
        lat_enabled: bool,
        cliff_back_impassability: u8,
        scenario_fill_ranged: &mut dyn FnMut(u32, u32) -> u32,
        variant_selector: &mut TileVariantSelectionContext<'_, '_>,
        shared_cell_dummy: SharedCellDummy,
        overlay_load_source: OverlayLoadSource,
        pixel_conversion_bounds: PixelConversionBounds,
    ) -> Self {
        Self::build_inner(
            map,
            theater_data,
            asset_manager,
            terrain_rules,
            overlay_registry,
            terrain_object_types,
            lat_enabled,
            cliff_back_impassability,
            overlay_load_source,
            TerrainBuildProjection::EagerCompatibility,
            Some(scenario_fill_ranged),
            Some(variant_selector),
            Some(shared_cell_dummy),
            pixel_conversion_bounds,
        )
    }

    /// Active authored-map Fill boundary. This materializes the native Size
    /// diamond, pristine TMP metadata, and variant selection only. Executable
    /// map-load effects are deliberately deferred until the staged Simulation
    /// owns native identity and can run the consuming overlay transaction.
    pub fn build_pending_authored_with_variant_selector_and_shared_dummy(
        map: &MapFile,
        theater_data: Option<&TheaterData>,
        asset_manager: Option<&crate::assets::asset_manager::AssetManager>,
        terrain_rules: Option<&TerrainRules>,
        overlay_registry: Option<&OverlayTypeRegistry>,
        lat_enabled: bool,
        cliff_back_impassability: u8,
        scenario_fill_ranged: &mut dyn FnMut(u32, u32) -> u32,
        variant_selector: &mut TileVariantSelectionContext<'_, '_>,
        shared_cell_dummy: SharedCellDummy,
    ) -> AuthoredTerrainFill {
        AuthoredTerrainFill(Self::build_inner(
            map,
            theater_data,
            asset_manager,
            terrain_rules,
            overlay_registry,
            None,
            lat_enabled,
            cliff_back_impassability,
            OverlayLoadSource::Authored,
            TerrainBuildProjection::PendingAuthored,
            Some(scenario_fill_ranged),
            Some(variant_selector),
            Some(shared_cell_dummy),
            PixelConversionBounds::default(),
        ))
    }

    pub(super) fn build_inner(
        map: &MapFile,
        theater_data: Option<&TheaterData>,
        asset_manager: Option<&crate::assets::asset_manager::AssetManager>,
        terrain_rules: Option<&TerrainRules>,
        overlay_registry: Option<&OverlayTypeRegistry>,
        terrain_object_types: Option<&HashMap<String, TerrainObjectType>>,
        lat_enabled: bool,
        cliff_back_impassability: u8,
        overlay_load_source: OverlayLoadSource,
        projection: TerrainBuildProjection,
        scenario_fill_ranged: Option<&mut dyn FnMut(u32, u32) -> u32>,
        mut variant_selector: Option<&mut TileVariantSelectionContext<'_, '_>>,
        shared_cell_dummy: Option<SharedCellDummy>,
        pixel_conversion_bounds: PixelConversionBounds,
    ) -> Self {
        let shared_cell_dummy = shared_cell_dummy.unwrap_or_default();
        let clear_tile_id = theater_data
            .and_then(|td| td.rmg_tiles.clear_tile)
            .unwrap_or(0);
        let materialized_size_diamond = variant_selector.is_some();
        let playfield = materialized_size_diamond.then(|| Playfield::from_header(&map.header));
        let raw_cells = if let (Some(selector), Some(fill_ranged)) =
            (variant_selector.as_deref_mut(), scenario_fill_ranged)
        {
            materialize_map_load_cells(
                map,
                theater_data.map(|theater| &theater.lookup),
                selector,
                fill_ranged,
                &shared_cell_dummy,
            )
        } else {
            // The selector-free constructor is retained for focused tests and
            // synthetic grids. Pack-derived fixtures still cross the native
            // compatibility seam, while manual/RMG actual IDs stay untouched.
            selector_free_map_cells(map, theater_data.map(|theater| &theater.lookup))
        };
        let (width, height) = grid_dimensions(&raw_cells);
        if width == 0 || height == 0 {
            let mut grid = Self {
                width: 0,
                height: 0,
                cells: Vec::new(),
                land_speed_costs: land_speed_cost_profiles(None),
                native_tmp_draw_heights: HashMap::new(),
                shared_cell_dummy,
                mutation_epoch: 0,
                native_allocated: materialized_size_diamond.then(Vec::new),
                radar_color_valid: Vec::new(),
                damaged_radar_metadata: Vec::new(),
                tube_facts: Vec::new(),
                native_tube_indices: Vec::new(),
                tube_native_ids: Vec::new(),
                clear_tile_id: 0,
                water_tiles: crate::map::rmg::tiles::TileIds::from_keys(&Default::default()),
                tile_registry_len: None,
                dummy_accepts_smudge: false,
                high_bridge_rim_tiles: None,
                bridge_set_start: None,
                wood_bridge_set_start: None,
                tile_animations: Vec::new(),
                destroyable_cliff_catalog: None,
                bridge_recalc_catalog: None,
                allocated_cell_order: Default::default(),
            };
            grid.bind_asset_catalogs(
                theater_data,
                asset_manager,
                terrain_rules,
                lat_enabled,
                cliff_back_impassability,
                variant_selector
                    .as_ref()
                    .map_or_else(Default::default, |s| s.selector()),
            );
            return grid;
        }

        let mut final_cells: Vec<MapCell> = raw_cells.clone();
        let mut metadata_cache: HashMap<TileKey, TileMetadata> = HashMap::new();
        let mut warned_unknown_land_types: HashSet<u8> = HashSet::new();
        let native_allocated = materialized_size_diamond.then(|| {
            let mut mask = vec![false; width as usize * height as usize];
            for cell in &raw_cells {
                let index = cell.ry as usize * width as usize + cell.rx as usize;
                if let Some(slot) = mask.get_mut(index) {
                    *slot = true;
                }
            }
            mask
        });
        let load_slope_states = if projection.is_eager() && lat_enabled {
            theater_data.map(|td| {
                let lat_config = lat::parse_lat_config(&td.ini_data, &td.lookup);
                let slope_config = lat::SlopeFixupConfig {
                    ramp_base: td.rmg_tiles.ramp_base.map_or(-1, i32::from),
                    ramp_smooth: td.rmg_tiles.ramp_smooth.map_or(-1, i32::from),
                };
                let mut pristine_slope = |tile_index: i32, sub_tile: u8| {
                    let Ok(tile_id) = u16::try_from(tile_index) else {
                        return 0;
                    };
                    if tile_index == theater::NO_TILE || usize::from(tile_id) >= td.lookup.len() {
                        return 0;
                    }
                    cached_tile_metadata(
                        &mut metadata_cache,
                        theater_data,
                        asset_manager,
                        terrain_rules,
                        TileKey {
                            tile_id,
                            sub_tile,
                            variant: 0,
                        },
                        &mut warned_unknown_land_types,
                    )
                    .slope_type
                };
                lat::apply_load_recalc_sweeps(
                    &mut final_cells,
                    &lat_config,
                    slope_config,
                    &mut pristine_slope,
                )
            })
        } else {
            None
        };
        let load_slope_lookup = load_slope_states.as_ref().map(|states| {
            final_cells
                .iter()
                .zip(states)
                .map(|(cell, &slope)| ((cell.rx, cell.ry), slope))
                .collect::<HashMap<_, _>>()
        });

        let raw_lookup: HashMap<(u16, u16), &MapCell> =
            raw_cells.iter().map(|c| ((c.rx, c.ry), c)).collect();
        let final_lookup: HashMap<(u16, u16), &MapCell> =
            final_cells.iter().map(|c| ((c.rx, c.ry), c)).collect();

        let snow_theater = map.header.theater.eq_ignore_ascii_case("SNOW");
        let terrain_objects: HashMap<(u16, u16), u8> = projection
            .is_eager()
            .then_some(map.terrain_objects.iter())
            .into_iter()
            .flatten()
            .map(|obj| {
                let occupation = terrain_object_types
                    .and_then(|types| types.get(&obj.name.to_ascii_uppercase()))
                    .map(|terrain_type| {
                        if snow_theater {
                            terrain_type.snow_occupation_bits
                        } else {
                            terrain_type.temperate_occupation_bits
                        }
                    })
                    .unwrap_or(7)
                    & 0x07;
                ((obj.rx, obj.ry), occupation)
            })
            .collect();

        let mut overlays_by_cell: HashMap<(u16, u16), Vec<&OverlayEntry>> = HashMap::new();
        for overlay in projection
            .is_eager()
            .then_some(map.overlays.iter())
            .into_iter()
            .flatten()
        {
            overlays_by_cell
                .entry((overlay.rx, overlay.ry))
                .or_default()
                .push(overlay);
        }

        let mut cells: Vec<ResolvedTerrainCell> =
            Vec::with_capacity(width as usize * height as usize);
        let mut radar_color_valid = Vec::with_capacity(cells.capacity());
        let mut damaged_radar_metadata = Vec::with_capacity(cells.capacity());
        let mut cliff_back_eligibility: Vec<CliffBackEligibility> =
            Vec::with_capacity(width as usize * height as usize);
        let mut tile_animations: Vec<TerrainTileAnimation> = Vec::new();
        let mut selector_calls = 0usize;
        let mut replacement_cells = 0usize;
        let mut high_suffix_cells = 0usize;
        let mut max_total_files = 1u8;

        for ry in 0..height {
            for rx in 0..width {
                let raw = raw_lookup.get(&(rx, ry)).copied();
                let final_cell = final_lookup.get(&(rx, ry)).copied();
                let (final_tile_index, final_sub_tile, level) = final_cell
                    .map(|cell| (cell.tile_index, cell.sub_tile, cell.z))
                    .unwrap_or((0, 0, 0));
                let tile_index_out_of_range = theater_data.is_some_and(|td| {
                    final_tile_index >= 0
                        && final_tile_index != 0xFFFF
                        && final_tile_index as usize >= td.lookup.len()
                });
                let uses_clear_fallback = raw.is_none()
                    || final_tile_index < 0
                    || final_tile_index == 0xFFFF
                    || tile_index_out_of_range;
                let (presentation_tile_id, presentation_sub_tile) = if uses_clear_fallback {
                    (clear_tile_id, 0)
                } else {
                    (normalize_tile_id(final_tile_index), final_sub_tile)
                };
                let pristine_key = TileKey {
                    tile_id: presentation_tile_id,
                    sub_tile: presentation_sub_tile,
                    variant: 0,
                };
                let pristine_metadata = cached_tile_metadata(
                    &mut metadata_cache,
                    theater_data,
                    asset_manager,
                    terrain_rules,
                    pristine_key,
                    &mut warned_unknown_land_types,
                );
                // The active RecalcAttributes caller validates this exact
                // registered pristine receiver. A failed entry check on an
                // otherwise valid tile takes the dedicated FFFF/0 fallback;
                // an already-invalid tile id instead stays on the later
                // sentinel branch.
                let pristine_subtile_entry_failed =
                    !uses_clear_fallback && pristine_metadata.subtile_entry_valid == Some(false);
                // Retail selects the independent tactical owner early, using
                // only pristine dimensions and the pristine damaged-data gate.
                // CellClass attributes remain owned by the registered pristine
                // head; selected-owner presentation fields are overlaid below.
                let mut variant = 0u8;
                if let (Some(td), Some(selector)) = (theater_data, variant_selector.as_mut()) {
                    let total_file_count = td.lookup.total_file_count(presentation_tile_id);
                    let outside_size_diamond = materialized_size_diamond && raw.is_none();
                    if !outside_size_diamond
                        && !pristine_subtile_entry_failed
                        && ordinary_variant_selection_enabled(
                            total_file_count,
                            uses_clear_fallback,
                            pristine_metadata.has_damaged_data,
                        )
                    {
                        max_total_files = max_total_files.max(total_file_count);
                        variant = selector.select_variant(
                            i32::from(rx),
                            i32::from(ry),
                            presentation_sub_tile,
                            pristine_metadata.template_width_cells,
                            pristine_metadata.template_height_cells,
                            total_file_count,
                        );
                        selector_calls += 1;
                        replacement_cells += usize::from(variant > 0);
                        high_suffix_cells += usize::from(variant > 4);
                    }
                }
                let selected_key = TileKey {
                    variant,
                    ..pristine_key
                };
                let mut metadata = pristine_metadata;
                if selected_key != pristine_key {
                    let selected_metadata = cached_tile_metadata(
                        &mut metadata_cache,
                        theater_data,
                        asset_manager,
                        terrain_rules,
                        selected_key,
                        &mut warned_unknown_land_types,
                    );
                    apply_selected_presentation_metadata(&mut metadata, &selected_metadata);
                }
                if !pristine_subtile_entry_failed
                    && let Some(slope_type) = load_slope_lookup
                        .as_ref()
                        .and_then(|slopes| slopes.get(&(rx, ry)))
                        .copied()
                {
                    metadata.slope_type = slope_type;
                    metadata.has_ramp = slope_type != 0;
                }
                let damaged_radar = damaged_variant_radar_metadata(
                    &mut metadata_cache,
                    theater_data,
                    asset_manager,
                    terrain_rules,
                    pristine_key,
                    &metadata,
                    &mut warned_unknown_land_types,
                );
                let terrain_object_occupation = terrain_objects.get(&(rx, ry)).copied();
                let terrain_object_blocks =
                    terrain_object_occupation.is_some_and(|occupation| occupation != 0);
                let overlay_effects = classify_overlay_effects(
                    overlays_by_cell.get(&(rx, ry)),
                    overlay_registry,
                    level,
                    metadata.slope_type,
                );
                let sparse_subtile_fallback =
                    pristine_subtile_entry_failed && !overlay_effects.claims_cell_attributes;
                if sparse_subtile_fallback {
                    // 0x0047D5E6..0x0047D5F9 materializes the failed valid-tile
                    // entry as a no-tile Cell before any valid-tile-only work.
                    metadata.tileset_index = None;
                    metadata.slope_type = 0;
                    metadata.has_ramp = false;
                }
                // Tile-attached animation. The engine spawns one AnimClass per
                // cell whose tile declares a `Tile%02dAnim` block and whose
                // sub-tile equals that block's `AttachesTo`, latching a per-cell
                // flag so later attribute recomputes never spawn a second one —
                // this single pass over each cell is that latch. Two earlier
                // returns exclude cells here too: a missing or out-of-range tile
                // id, and an overlay that claims the cell's attributes.
                if projection.is_eager()
                    && !uses_clear_fallback
                    && !sparse_subtile_fallback
                    && !overlay_effects.claims_cell_attributes
                    && let Some(anim) =
                        theater_data.and_then(|td| td.lookup.tile_anim(presentation_tile_id))
                    && anim.attaches_to == i32::from(presentation_sub_tile)
                {
                    let (offset_x, offset_y) =
                        pixel_conversion_bounds.offset_to_leptons(anim.x_offset, anim.y_offset);
                    tile_animations.push(TerrainTileAnimation {
                        rx,
                        ry,
                        anim_name: anim.anim_name.clone(),
                        world_x: offset_x
                            .wrapping_add(i32::from(rx).wrapping_mul(LEPTONS_PER_CELL))
                            .wrapping_add(CELL_CENTRE_LEPTONS),
                        world_y: offset_y
                            .wrapping_add(i32::from(ry).wrapping_mul(LEPTONS_PER_CELL))
                            .wrapping_add(CELL_CENTRE_LEPTONS),
                        world_z: i32::from(level as i8)
                            * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS,
                        z_adjust: anim.z_adjust,
                    });
                }
                let canonical_ramp = canonical_ramp_from_slope_type(metadata.slope_type);
                // The registered pristine TMP owns the base snapshot. Overlay
                // land never feeds back into these restoration fields.
                let base_land_type = metadata.land_type;
                let base_yr_cell_land_type = metadata.yr_cell_land_type;
                let base_terrain_class = metadata.terrain_class;
                let base_speed_costs = metadata.speed_costs;
                let base_ground_walk_blocked = canonical_ramp.is_none() && metadata.ground_blocked;
                let base_build_blocked = metadata.build_blocked || canonical_ramp.is_some();
                if !sparse_subtile_fallback && let Some(land) = overlay_effects.effective_land {
                    apply_canonical_land_to_metadata(
                        &mut metadata,
                        land,
                        overlay_effects.effective_land_speed_costs,
                        overlay_effects.effective_land_ground_blocked,
                    );
                }
                let base_cliff_back_eligible = if sparse_subtile_fallback {
                    base_land_type == LandType::Clear.as_index()
                } else {
                    cliff_back_normal_reclass_applies(base_land_type)
                };
                let current_cliff_back_eligible = if overlay_effects.claims_cell_attributes {
                    true
                } else if sparse_subtile_fallback {
                    metadata.land_type == LandType::Clear.as_index()
                } else {
                    cliff_back_normal_reclass_applies(metadata.land_type)
                };
                cliff_back_eligibility.push(CliffBackEligibility {
                    current: current_cliff_back_eligible,
                    base: base_cliff_back_eligible,
                });
                let is_cliff_like = metadata.is_cliff_like;
                let outside = playfield.is_some_and(|playfield| {
                    !playfield.contains_raised(rx, ry, level as i8, metadata.slope_type)
                });
                let zone_type = recalc_zone_type(
                    outside,
                    overlay_effects.overlay_zone_type,
                    metadata.land_type,
                    metadata.speed_costs.wheel,
                    terrain_object_occupation,
                );
                // Same shape as `base_ground_walk_blocked`, but on the metadata
                // *after* any overlay land override. `base_*` stays pristine on
                // purpose — it is the restoration value for overlay removal —
                // so reading it here would pin a low bridge's deck to the
                // water underneath it.
                let ground_walk_blocked = (canonical_ramp.is_none() && metadata.ground_blocked)
                    || terrain_object_blocks
                    || overlay_effects.overlay_blocks;
                let bridge_walkable = overlay_effects.has_bridge_deck
                    && !overlay_effects.is_low_bridge
                    && !terrain_object_blocks
                    && !overlay_effects.overlay_blocks;
                // Smudges (craters, scorches) only place on tiles whose tileset has
                // Morphable=yes. Cells with no resolved tile (filled_clear) default
                // to false. Computed once at resolve time so the smudge dispatcher
                // reads a single bool.
                let accepts_smudge = if uses_clear_fallback || sparse_subtile_fallback {
                    false
                } else {
                    theater_data
                        .map(|td| td.lookup.is_morphable(presentation_tile_id))
                        .unwrap_or(false)
                };
                let allows_tiberium = if uses_clear_fallback || sparse_subtile_fallback {
                    false
                } else {
                    theater_data
                        .map(|td| td.lookup.allows_tiberium(presentation_tile_id))
                        .unwrap_or(false)
                };
                // Allow layer transitions on any bridge deck cell. High bridges over
                // water have ground_walk_blocked=true, but units still need to transition
                // from Ground→Bridge at the ramp/entry cells.
                // Only bridgehead ramp cells (detected below) allow layer
                // transitions. Deck cells must NOT be transitions — otherwise
                // the A* can switch Bridge→Ground mid-span and units clip
                // through the bridge.
                let bridge_transition = false;
                let stored_final_tile_index = if sparse_subtile_fallback {
                    0xFFFF
                } else {
                    final_tile_index
                };
                let stored_final_sub_tile = if sparse_subtile_fallback {
                    0
                } else {
                    final_sub_tile
                };
                let is_wood_bridge_repair_tile =
                    is_wood_bridge_repair_tile(theater_data, stored_final_tile_index);
                radar_color_valid.push(metadata.subtile_entry_valid == Some(true));
                damaged_radar_metadata.push(damaged_radar.map(Ok));
                cells.push(ResolvedTerrainCell {
                    rx,
                    ry,
                    source_tile_index: raw.map(|c| c.tile_index).unwrap_or(theater::NO_TILE),
                    source_sub_tile: raw.map(|c| c.sub_tile).unwrap_or(0),
                    final_tile_index: stored_final_tile_index,
                    final_sub_tile: stored_final_sub_tile,
                    is_wood_bridge_repair_tile,
                    level,
                    filled_clear: raw.is_none(),
                    tileset_index: metadata.tileset_index,
                    land_type: metadata.land_type,
                    yr_cell_land_type: metadata.yr_cell_land_type,
                    slope_type: metadata.slope_type,
                    template_height: metadata.template_height,
                    height_in_pixels: metadata.height_in_pixels,
                    render_offset_x: metadata.render_offset_x,
                    render_offset_y: metadata.render_offset_y,
                    terrain_class: metadata.terrain_class,
                    speed_costs: metadata.speed_costs,
                    is_water: metadata.is_water,
                    is_cliff_like,
                    is_rough: metadata.is_rough,
                    is_road: metadata.is_road,
                    accepts_smudge,
                    allows_tiberium,
                    variant,
                    has_ramp: metadata.has_ramp,
                    canonical_ramp,
                    ground_walk_blocked,
                    terrain_object_blocks,
                    terrain_object_occupation,
                    overlay_blocks: overlay_effects.overlay_blocks,
                    overlay_zone_type: overlay_effects.overlay_zone_type,
                    outside_playfield: outside,
                    zone_type,
                    base_ground_walk_blocked,
                    base_build_blocked,
                    base_land_type,
                    base_yr_cell_land_type,
                    base_terrain_class,
                    base_speed_costs,
                    has_bridge_deck: overlay_effects.has_bridge_deck,
                    bridge_walkable,
                    bridge_transition,
                    bridge_deck_level: overlay_effects
                        .bridge_layer
                        .as_ref()
                        .map(|layer| layer.deck_level)
                        .unwrap_or(level),
                    bridge_layer: overlay_effects.bridge_layer,
                    bridge_facts: BridgeCellFacts::default(),
                    tube_index: None,
                    radar_left: metadata.radar_left,
                    radar_right: metadata.radar_right,
                    has_damaged_data: metadata.has_damaged_data,
                });
            }
        }
        // The engine reaches cells through an anti-diagonal iterator, so the
        // animations are constructed in that order. The loop above is row-major,
        // so restore the sweep order here: the spawner then assigns animation
        // identities the way a native load would.
        tile_animations.sort_by_key(|anim| (anim.rx as u32 + anim.ry as u32, anim.rx));
        if !tile_animations.is_empty() {
            log::info!(
                "ResolvedTerrain: {} terrain tile animations resolved",
                tile_animations.len(),
            );
        }
        if selector_calls > 0 {
            log::info!(
                "ResolvedTerrain variants: {} selector cells, {} replacements, {} e-or-later, max total files {}",
                selector_calls,
                replacement_cells,
                high_suffix_cells,
                max_total_files,
            );
        }

        // OverlayPack is decoded in fixed-grid row-major order. Each live
        // OverlayClass anchor is an allocated CellClass; its setter then walks
        // through MapClass lookups, so missing neighbors stamp/mutate the one
        // shared dummy rather than disappearing at the rectangular edge.
        for overlay in projection
            .is_eager()
            .then_some(map.overlays.iter())
            .into_iter()
            .flatten()
        {
            let anchor_index = native_resolved_cell_index(
                width,
                height,
                native_allocated.as_deref(),
                cells.len(),
                i32::from(overlay.rx),
                i32::from(overlay.ry),
            );
            if let Some(index) = anchor_index {
                cells[index].bridge_facts.overlay_id = Some(overlay.overlay_id);
            }
            if overlay_load_source == OverlayLoadSource::Authored
                && let Some((family, direction)) =
                    crate::map::bridge_facts::high_bridge_stamp_for_overlay(overlay.overlay_id)
                && anchor_index.is_some()
            {
                let stamp = BridgeFlagStamp::new((overlay.rx, overlay.ry), direction, true);
                let _ = apply_native_bridge_flag_stamp_to_parts(
                    &mut cells,
                    width,
                    height,
                    native_allocated.as_deref(),
                    &shared_cell_dummy,
                    stamp,
                    Some(family),
                );
            }
        }

        let raw_overlay_data = projection.is_eager().then(|| {
            map.overlay_data_pack()
                .expect("legacy terrain projection must precede authored pack consumption")
        });
        if let Some(raw_overlay_data) = raw_overlay_data.filter(|data| data.is_present()) {
            for (index, cell) in cells.iter_mut().enumerate() {
                if native_allocated
                    .as_deref()
                    .is_none_or(|mask| mask.get(index).copied().unwrap_or(false))
                {
                    cell.bridge_facts.state_byte = raw_overlay_data.byte_at(cell.rx, cell.ry);
                }
            }
        }

        for cell in projection
            .is_eager()
            .then_some(cells.iter_mut())
            .into_iter()
            .flatten()
        {
            let facts = cell.bridge_facts;
            if facts.has_structural_bridge() {
                cell.has_bridge_deck = true;
                cell.bridge_walkable = !cell.terrain_object_blocks && !cell.overlay_blocks;
                cell.bridge_deck_level = cell.level.saturating_add(4);
            } else if facts.family != crate::map::bridge_facts::BridgeStampFamily::None
                && cell
                    .bridge_layer
                    .as_ref()
                    .is_some_and(|bl| bl.direction != BridgeDirection::Low)
            {
                cell.has_bridge_deck = false;
                cell.bridge_walkable = false;
                cell.bridge_deck_level = cell.level;
            }

            if facts.has_transition_flag() {
                cell.bridge_transition = true;
            }
        }

        {
            let mut high_deck: Vec<(u16, u16, u8, u32)> = cells
                .iter()
                .filter(|c| c.bridge_facts.has_structural_bridge())
                .map(|c| (c.rx, c.ry, c.bridge_deck_level, c.bridge_facts.raw_flags))
                .collect();
            high_deck.sort_by_key(|(rx, ry, _, _)| (*rx, *ry));
            if !high_deck.is_empty() {
                log::debug!(
                    "High bridge stamped structural cells ({} total):",
                    high_deck.len(),
                );
                for (rx, ry, dl, flags) in &high_deck {
                    log::debug!("  ({}, {}) deck_level={} flags=0x{:X}", rx, ry, dl, flags);
                }
            }
        }

        // Log bridge cell statistics for diagnostics.
        let bridge_cell_count: usize = cells.iter().filter(|c| c.has_bridge_deck).count();
        let low_bridge_count: usize = cells
            .iter()
            .filter(|c| {
                c.bridge_layer
                    .as_ref()
                    .map(|bl| bl.direction == BridgeDirection::Low)
                    .unwrap_or(false)
            })
            .count();
        let high_bridge_count: usize = bridge_cell_count - low_bridge_count;
        if bridge_cell_count > 0 {
            log::info!(
                "ResolvedTerrain: {} bridge deck cells ({} high, {} low)",
                bridge_cell_count,
                high_bridge_count,
                low_bridge_count,
            );
        }

        // CellClass::RecalcAttributes has three CliffBack writer gates: early
        // overlay is unconditional, unusable/sparse subtile is Clear-only, and
        // the normal path accepts Clear, Water, Beach, or Ice. Non-2 bytes have
        // no observable write, so this load resolver output-gates the pure scan.
        if projection.is_eager() && cliff_back_impassability == 2 {
            const CLIFF_BACK_HEIGHT_DIFF: i16 = 4;
            // 6 neighbor offsets in (dx, dy) matching gamemd.exe RecalcAttributes:
            // (X, Y-1), (X-1, Y), (X+2, Y+2), (X+1, Y+1), (X-1, Y+1), (X+1, Y-1)
            const NEIGHBOR_OFFSETS: [(i32, i32); 6] =
                [(0, -1), (-1, 0), (2, 2), (1, 1), (-1, 1), (1, -1)];
            let rock_lt = LandType::Rock.as_index();
            let mut rock_terrain_class = LandType::Rock.terrain_class();
            let mut rock_speed_costs = SpeedCostProfile {
                wheel: Some(0),
                ..Default::default()
            };
            let mut rock_is_water = LandType::Rock.is_water();
            let mut rock_is_rough = LandType::Rock.is_rough();
            let mut rock_is_road = LandType::Rock.is_road();
            let mut rock_ground_blocked = true;
            let mut rock_build_blocked = true;
            if let Some(rock) = terrain_rules
                .and_then(|rules| rules.semantics_for_land_type(LandType::Rock.as_index()))
            {
                rock_terrain_class = rock.terrain_class;
                rock_speed_costs = rock.speed_costs;
                rock_is_water = rock.water;
                rock_is_rough = rock.rough;
                rock_is_road = rock.road;
                rock_ground_blocked = rock.ground_blocked;
                rock_build_blocked = !rock.buildable;
            }

            debug_assert_eq!(cliff_back_eligibility.len(), cells.len());
            let resolved_cell_count = cells.len();
            let fixed_stride_index = |x: i32, y: i32| {
                let native_index = crate::map::cell_index::cell_linear_index(x, y)?;
                let canonical_x = (native_index % crate::map::cell_index::CELL_ROW_STRIDE) as usize;
                let canonical_y = (native_index / crate::map::cell_index::CELL_ROW_STRIDE) as usize;
                if canonical_x >= usize::from(width) || canonical_y >= usize::from(height) {
                    return None;
                }
                let resolved_index = canonical_y * usize::from(width) + canonical_x;
                (resolved_index < resolved_cell_count
                    && native_allocated
                        .as_deref()
                        .is_none_or(|mask| mask.get(resolved_index).copied().unwrap_or(false)))
                .then_some(resolved_index)
            };

            let mut cliff_back_count: usize = 0;
            for idx in 0..cells.len() {
                let eligibility = cliff_back_eligibility[idx];
                if !eligibility.current && !eligibility.base {
                    continue;
                }
                let cell_level = i16::from(cells[idx].level as i8);
                let rx = cells[idx].rx as i16;
                let ry = cells[idx].ry as i16;

                let mut behind_cliff = false;
                for &(dx, dy) in &NEIGHBOR_OFFSETS {
                    let nx = rx.wrapping_add(dx as i16) as i32;
                    let ny = ry.wrapping_add(dy as i16) as i32;
                    if fixed_stride_index(nx, ny).is_some_and(|nidx| {
                        i16::from(cells[nidx].level as i8) >= cell_level + CLIFF_BACK_HEIGHT_DIFF
                    }) {
                        behind_cliff = true;
                        break;
                    }
                }
                if behind_cliff {
                    let cell = &mut cells[idx];
                    if eligibility.current {
                        cell.land_type = rock_lt;
                        cell.yr_cell_land_type = rock_lt;
                        cell.terrain_class = rock_terrain_class;
                        cell.speed_costs = rock_speed_costs;
                        cell.is_water = rock_is_water;
                        cell.is_cliff_like = true;
                        cell.is_rough = rock_is_rough;
                        cell.is_road = rock_is_road;
                        // RecalcZoneType runs after the native LandType write.
                        cell.ground_walk_blocked = rock_ground_blocked
                            || cell.terrain_object_blocks
                            || cell.overlay_blocks;
                        cell.zone_type = recalc_zone_type(
                            cell.outside_playfield,
                            cell.overlay_zone_type,
                            cell.land_type,
                            cell.speed_costs.wheel,
                            cell.terrain_object_occupation,
                        );
                        cliff_back_count += 1;
                    }
                    // Restoration follows the no-overlay branch. A claiming
                    // overlay on underlying Road must not persist Rock after
                    // that overlay is removed.
                    if eligibility.base {
                        cell.base_land_type = rock_lt;
                        cell.base_yr_cell_land_type = rock_lt;
                        cell.base_terrain_class = rock_terrain_class;
                        cell.base_speed_costs = rock_speed_costs;
                        cell.base_ground_walk_blocked = rock_ground_blocked;
                        cell.base_build_blocked = rock_build_blocked;
                    }
                }
            }
            if cliff_back_count > 0 {
                log::info!(
                    "ResolvedTerrain: {} cells marked impassable by CliffBackImpassability",
                    cliff_back_count,
                );
            }
        }

        let mut tube_facts = Vec::new();
        if projection.is_eager() {
            tube_facts = seed_explicit_map_tubes(&mut cells, width, height, &map.explicit_tubes);
            build_auto_low_bridge_tubes(&mut cells, width, height, theater_data, &mut tube_facts);
        }
        let tube_native_ids = vec![None; tube_facts.len()];
        let native_tube_indices = cells
            .iter()
            .map(|cell| {
                cell.tube_index
                    .map_or(NativeTubeCellIndex::NONE, |tube_id| {
                        NativeTubeCellIndex::from_registry_ordinal(tube_id.as_usize())
                    })
            })
            .collect();

        let mut grid = Self {
            width,
            height,
            cells,
            land_speed_costs: land_speed_cost_profiles(None),
            native_tmp_draw_heights: HashMap::new(),
            shared_cell_dummy,
            mutation_epoch: 0,
            native_allocated,
            radar_color_valid,
            damaged_radar_metadata,
            tube_facts,
            native_tube_indices,
            tube_native_ids,
            clear_tile_id: 0,
            water_tiles: crate::map::rmg::tiles::TileIds::from_keys(&Default::default()),
            tile_registry_len: None,
            dummy_accepts_smudge: false,
            high_bridge_rim_tiles: None,
            bridge_set_start: None,
            wood_bridge_set_start: None,
            tile_animations,
            destroyable_cliff_catalog: None,
            bridge_recalc_catalog: None,
            allocated_cell_order: Default::default(),
        };
        grid.bind_asset_catalogs(
            theater_data,
            asset_manager,
            terrain_rules,
            lat_enabled,
            cliff_back_impassability,
            variant_selector
                .as_ref()
                .map_or_else(Default::default, |s| s.selector()),
        );
        grid
    }

    pub fn build_height_map(&self) -> BTreeMap<(u16, u16), u8> {
        self.cells
            .iter()
            .map(|cell| ((cell.rx, cell.ry), cell.level))
            .collect()
    }

    /// Build a bridge deck height map — only HIGH bridge cells are included.
    /// Low bridges (LOBRDG/LOBRDB) are at ground level and don't need height
    /// correction for click resolution or debug overlays.
    pub fn build_bridge_height_map(&self) -> BTreeMap<(u16, u16), u8> {
        self.cells
            .iter()
            .filter(|cell| {
                cell.has_bridge_deck
                    && !cell
                        .bridge_layer
                        .as_ref()
                        .is_some_and(|bl| bl.direction == BridgeDirection::Low)
            })
            .map(|cell| ((cell.rx, cell.ry), cell.bridge_deck_level))
            .collect()
    }
}

#[derive(Debug, Clone)]
pub(super) struct TileMetadata {
    pub(super) tileset_index: Option<u16>,
    pub(super) has_tmp_metadata: bool,
    /// The independent TMP file parsed successfully. This stays true for a
    /// sparse/missing requested subimage, which native distinguishes from an
    /// absent damaged sibling chain.
    pub(super) tmp_file_valid: bool,
    /// Result of the active CellClass subtile-entry check. `None` means this
    /// synthetic/tooling path had no TMP data source to check.
    pub(super) subtile_entry_valid: Option<bool>,
    /// Pristine TMP grid dimensions used by ordinary variant normalization.
    pub(super) template_width_cells: u32,
    pub(super) template_height_cells: u32,
    /// Canonical YR LandType used for terrain semantics and CellClass Land
    /// derivation. Reduced `zone_type` selects the movement matrix column.
    pub(super) land_type: u8,
    /// Retained CellClass LandType mirror for binary-derived predicates and
    /// compatibility consumers. This is not the raw TMP terrain_type byte.
    pub(super) yr_cell_land_type: u8,
    /// Raw TMP terrain_type byte (0-15) for rules.ini semantic lookups.
    pub(super) raw_land_type: u8,
    pub(super) slope_type: u8,
    pub(super) template_height: u8,
    pub(super) height_in_pixels: i8,
    pub(super) render_offset_x: i32,
    pub(super) render_offset_y: i32,
    pub(super) terrain_class: TerrainClass,
    pub(super) speed_costs: SpeedCostProfile,
    pub(super) is_water: bool,
    pub(super) is_cliff_like: bool,
    pub(super) is_rough: bool,
    pub(super) is_road: bool,
    pub(super) has_ramp: bool,
    pub(super) ground_blocked: bool,
    pub(super) build_blocked: bool,
    /// Per-tile radar minimap color (left half of isometric diamond), from TMP header.
    pub(super) radar_left: [u8; 3],
    /// Per-tile radar minimap color (right half of isometric diamond), from TMP header.
    pub(super) radar_right: [u8; 3],
    /// Mirrors `TmpTile.has_damaged_data` — set when the TMP sub-tile flag DWORD
    /// declares a baked damaged-variant pixel set.
    pub(super) has_damaged_data: bool,
}

impl Default for TileMetadata {
    fn default() -> Self {
        Self {
            tileset_index: None,
            has_tmp_metadata: false,
            tmp_file_valid: false,
            subtile_entry_valid: None,
            template_width_cells: 0,
            template_height_cells: 0,
            land_type: 0,
            yr_cell_land_type: 0,
            raw_land_type: 0,
            slope_type: 0,
            template_height: 0,
            height_in_pixels: 0,
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: TerrainClass::Unknown,
            speed_costs: SpeedCostProfile::default(),
            is_water: false,
            is_cliff_like: false,
            is_rough: false,
            is_road: false,
            has_ramp: false,
            ground_blocked: false,
            build_blocked: false,
            radar_left: [0, 0, 0],
            radar_right: [0, 0, 0],
            has_damaged_data: false,
        }
    }
}

impl TileMetadata {
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn from_resolved_cell(cell: &ResolvedTerrainCell) -> Self {
        Self {
            tileset_index: cell.tileset_index,
            has_tmp_metadata: true,
            tmp_file_valid: true,
            subtile_entry_valid: Some(true),
            template_width_cells: 1,
            template_height_cells: 1,
            land_type: cell.base_land_type,
            yr_cell_land_type: cell.base_yr_cell_land_type,
            raw_land_type: cell.base_land_type,
            slope_type: cell.slope_type,
            template_height: cell.template_height,
            height_in_pixels: cell.height_in_pixels,
            render_offset_x: cell.render_offset_x,
            render_offset_y: cell.render_offset_y,
            terrain_class: cell.base_terrain_class,
            speed_costs: cell.base_speed_costs,
            is_water: cell.base_terrain_class == TerrainClass::Water,
            is_cliff_like: cell.is_cliff_like,
            is_rough: cell.is_rough,
            is_road: cell.is_road,
            has_ramp: cell.has_ramp,
            ground_blocked: cell.base_ground_walk_blocked,
            build_blocked: cell.base_build_blocked,
            radar_left: cell.radar_left,
            radar_right: cell.radar_right,
            has_damaged_data: cell.has_damaged_data,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct OverlayEffects {
    /// The overlay owns this cell's attributes, so `RecalcAttributes` takes its
    /// short branch: LAT/slope fixup and the zone recompute still run, but the
    /// tile-attached animation spawn and the ShadowCaster walk are skipped.
    pub(super) claims_cell_attributes: bool,
    pub(super) overlay_blocks: bool,
    pub(super) overlay_zone_type: Option<u8>,
    pub(super) has_bridge_deck: bool,
    pub(super) bridge_layer: Option<BridgeLayer>,
    is_low_bridge: bool,
    pub(super) effective_land: Option<LandType>,
    pub(super) effective_land_speed_costs: Option<SpeedCostProfile>,
    /// Ground-blocking of `effective_land`'s rules row. Travels with
    /// `effective_land` so the replacement LandType brings its own passability
    /// instead of leaving the tile's behind. See `OverlayTypeFlags::land_ground_blocked`.
    pub(super) effective_land_ground_blocked: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct CliffBackEligibility {
    /// Eligibility of the current overlay-adjusted Cell LandType branch.
    current: bool,
    /// Eligibility of the no-overlay restoration snapshot.
    base: bool,
}

fn grid_dimensions(cells: &[MapCell]) -> (u16, u16) {
    let mut max_rx: u16 = 0;
    let mut max_ry: u16 = 0;
    let mut found = false;
    for cell in cells {
        found = true;
        max_rx = max_rx.max(cell.rx);
        max_ry = max_ry.max(cell.ry);
    }
    if found {
        (max_rx.saturating_add(1), max_ry.saturating_add(1))
    } else {
        (0, 0)
    }
}

fn normalize_tile_id(tile_index: i32) -> u16 {
    if tile_index == 0xFFFF || tile_index < 0 {
        0
    } else {
        tile_index as u16
    }
}

pub(super) fn presentation_tile_parts(
    tile_index: i32,
    sub_tile: u8,
    clear_tile_id: u16,
) -> (u16, u8) {
    if tile_index == 0xFFFF || tile_index < 0 {
        (clear_tile_id, 0)
    } else {
        (tile_index as u16, sub_tile)
    }
}

/// Land types the final normal `CliffBackImpassability` site reclasses to Rock.
///
/// The engine's filter on the ordinary (valid-tile, non-overlay) path is
/// exactly `Clear`, `Water`, `Beach` and `Ice`. Ice is reachable on Snow-theater
/// maps, where a frozen surface runs up to a cliff base; without it those cells
/// stay walkable and buildable where the engine blocks them.
///
/// The two sibling sites use different filters — the overlay-claimed path
/// reclasses unconditionally and the valid-tile sparse/OOB-subtile fallback
/// filters `Clear` only. An already-invalid/sentinel tile id reaches this
/// final-normal filter, not the sparse-entry copy.
pub(super) fn cliff_back_normal_reclass_applies(land_type: u8) -> bool {
    use crate::rules::terrain_rules::LandType;
    land_type == LandType::Clear.as_index()
        || land_type == LandType::Water.as_index()
        || land_type == LandType::Beach.as_index()
        || land_type == LandType::Ice.as_index()
}

pub(super) fn apply_load_land_to_cell(
    cell: &mut ResolvedTerrainCell,
    land: LandType,
    speed_costs: Option<SpeedCostProfile>,
    ground_blocked: bool,
) {
    cell.land_type = land.as_index();
    cell.yr_cell_land_type = land.as_index();
    cell.terrain_class = land.terrain_class();
    cell.speed_costs = speed_costs.unwrap_or_default();
    cell.is_water = land.is_water();
    cell.is_cliff_like = land.is_cliff_like();
    cell.is_rough = land.is_rough();
    cell.is_road = land.is_road();
    cell.ground_walk_blocked = ground_blocked;
}

pub(super) fn restore_load_base_land(cell: &mut ResolvedTerrainCell) {
    cell.land_type = cell.base_land_type;
    cell.yr_cell_land_type = cell.base_yr_cell_land_type;
    cell.terrain_class = cell.base_terrain_class;
    cell.speed_costs = cell.base_speed_costs;
    cell.is_water = cell.base_land_type == LandType::Water.as_index();
    cell.is_cliff_like = matches!(
        cell.base_land_type,
        land if land == LandType::Rock.as_index() || land == LandType::Wall.as_index()
    );
    cell.is_rough = cell.base_land_type == LandType::Rough.as_index();
    cell.is_road = cell.base_land_type == LandType::Road.as_index();
}

/// Current Cell+38 queries, separate from Recalc's retained pristine metadata.
/// Native smudge6B601A..6B603A falls back to tile0 for an invalid signed index;
/// CanPlaceTiberium4839C0..4839E9 admits that index at its final tile gate.
/// Other placement gates are outside this projection. Original-block witnesses:
/// tools/spatial_oracle/terrain_tile_permissions.
pub fn current_tile_permissions(lookup: &TilesetLookup, tile: i32) -> (bool, bool) {
    if tile < 0 || tile as usize >= lookup.len() {
        (lookup.is_morphable(0), true)
    } else {
        // The registry admits at most65535 slots; compare before narrowing.
        let tile = tile as u16;
        (lookup.is_morphable(tile), lookup.allows_tiberium(tile))
    }
}

pub(super) fn apply_pristine_load_metadata(
    cell: &mut ResolvedTerrainCell,
    metadata: &TileMetadata,
    accepts_smudge: bool,
    allows_tiberium: bool,
) {
    cell.tileset_index = metadata.tileset_index;
    cell.land_type = metadata.land_type;
    cell.yr_cell_land_type = metadata.yr_cell_land_type;
    cell.slope_type = metadata.slope_type;
    cell.template_height = metadata.template_height;
    cell.height_in_pixels = metadata.height_in_pixels;
    cell.terrain_class = metadata.terrain_class;
    cell.speed_costs = metadata.speed_costs;
    cell.is_water = metadata.is_water;
    cell.is_cliff_like = metadata.is_cliff_like;
    cell.is_rough = metadata.is_rough;
    cell.is_road = metadata.is_road;
    cell.accepts_smudge = accepts_smudge;
    cell.allows_tiberium = allows_tiberium;
    cell.has_ramp = metadata.has_ramp;
    cell.canonical_ramp = canonical_ramp_from_slope_type(metadata.slope_type);
    cell.base_land_type = metadata.land_type;
    cell.base_yr_cell_land_type = metadata.yr_cell_land_type;
    cell.base_terrain_class = metadata.terrain_class;
    cell.base_speed_costs = metadata.speed_costs;
    cell.base_ground_walk_blocked = cell.canonical_ramp.is_none() && metadata.ground_blocked;
    cell.base_build_blocked = metadata.build_blocked || cell.canonical_ramp.is_some();
}

pub(super) fn ordinary_variant_selection_enabled(
    total_file_count: u8,
    uses_clear_fallback: bool,
    has_damaged_data: bool,
) -> bool {
    !has_damaged_data && total_file_count != 0 && (uses_clear_fallback || total_file_count > 1)
}

pub(super) fn is_wood_bridge_repair_tile(
    theater_data: Option<&TheaterData>,
    final_tile_index: i32,
) -> bool {
    if final_tile_index < 0 || final_tile_index == 0xFFFF {
        return false;
    }
    let Some(td) = theater_data else {
        return false;
    };
    let Some(wood_bridge_set) = td.wood_bridge_set else {
        return false;
    };
    let Some(bounds) = td.lookup.bounds().get(wood_bridge_set as usize) else {
        return false;
    };
    let tile_id = normalize_tile_id(final_tile_index) as u32;
    let start = bounds.start as u32;
    tile_id >= start && tile_id < start + 16
}

/// A small asset-derived type catalogue, independent of visited cell identities.
/// Include every registered pristine type so LAT, cliff collapse and restore can
/// change a cell's tile without a render-thread asset lookup or stale scalar.
/// Only headers are read here; the large color/depth planes are not decoded.
pub(super) fn load_native_tmp_draw_heights(
    theater: Option<&TheaterData>,
    assets: Option<&crate::assets::asset_manager::AssetManager>,
) -> HashMap<u16, PristineTmpHeader> {
    let (Some(theater), Some(assets)) = (theater, assets) else {
        return HashMap::new();
    };
    let mut heights = HashMap::new();
    for id in 0..theater.lookup.len() {
        let Ok(id) = u16::try_from(id) else {
            break;
        };
        let Some(name) = theater.lookup.filename(i32::from(id)) else {
            log::warn!("TMP draw dimensions unavailable: tile {id} has no registered filename");
            continue;
        };
        let Some(bytes) = assets.get(name) else {
            log::warn!("TMP draw dimensions unavailable: tile {id} asset {name} is missing");
            continue;
        };
        match TmpFile::pristine_header_fields_from_bytes(&bytes) {
            Ok(rows) => {
                heights.insert(
                    id,
                    PristineTmpHeader {
                        subtiles: rows,
                        total_file_count: theater.lookup.total_file_count(id) as usize,
                    },
                );
            }
            Err(error) => log::warn!("TMP draw dimensions unavailable for {name}: {error}"),
        }
    }
    heights
}

pub(super) fn cached_tile_metadata(
    cache: &mut HashMap<TileKey, TileMetadata>,
    theater_data: Option<&TheaterData>,
    asset_manager: Option<&crate::assets::asset_manager::AssetManager>,
    terrain_rules: Option<&TerrainRules>,
    key: TileKey,
    warned_unknown_land_types: &mut HashSet<u8>,
) -> TileMetadata {
    if let Some(metadata) = cache.get(&key) {
        return metadata.clone();
    }
    let metadata = load_tile_metadata(
        theater_data,
        asset_manager,
        terrain_rules,
        key,
        warned_unknown_land_types,
    );
    cache.insert(key, metadata.clone());
    metadata
}

/// Apply only fields owned by the selected independent tactical TMP. Pixel
/// bounds, Z, and extra planes stay atlas-owned; this resolver carries the
/// selected cell's radar pair and render-origin offsets. Every CellClass field
/// remains on `pristine`.
pub(super) fn apply_selected_presentation_metadata(
    pristine: &mut TileMetadata,
    selected: &TileMetadata,
) {
    pristine.radar_left = selected.radar_left;
    pristine.radar_right = selected.radar_right;
    pristine.subtile_entry_valid = selected.subtile_entry_valid;
    pristine.render_offset_x = selected.render_offset_x;
    pristine.render_offset_y = selected.render_offset_y;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadarColorMetadata {
    pub left: [u8; 3],
    pub right: [u8; 3],
    pub valid: bool,
}

fn damaged_variant_radar_metadata(
    cache: &mut HashMap<TileKey, TileMetadata>,
    theater_data: Option<&TheaterData>,
    asset_manager: Option<&crate::assets::asset_manager::AssetManager>,
    terrain_rules: Option<&TerrainRules>,
    pristine_key: TileKey,
    pristine: &TileMetadata,
    warned_unknown_land_types: &mut HashSet<u8>,
) -> Option<RadarColorMetadata> {
    if !pristine.has_damaged_data
        || !theater_data
            .is_some_and(|theater| theater.lookup.total_file_count(pristine_key.tile_id) >= 2)
    {
        return None;
    }
    let damaged = cached_tile_metadata(
        cache,
        theater_data,
        asset_manager,
        terrain_rules,
        TileKey {
            variant: 1,
            ..pristine_key
        },
        warned_unknown_land_types,
    );
    // A missing/corrupt independent file never enters native's variant chain,
    // so the selector wraps to pristine. A parsed file with no requested
    // subimage does enter the chain and GetRadarColor returns fixed gray.
    retained_damaged_radar_metadata(pristine, Some(&damaged))
}

pub(super) fn retained_damaged_radar_metadata(
    pristine: &TileMetadata,
    damaged: Option<&TileMetadata>,
) -> Option<RadarColorMetadata> {
    let damaged =
        damaged.filter(|metadata| pristine.has_damaged_data && metadata.tmp_file_valid)?;
    Some(RadarColorMetadata {
        left: damaged.radar_left,
        right: damaged.radar_right,
        valid: damaged.subtile_entry_valid == Some(true),
    })
}

pub(super) fn load_tile_metadata(
    theater_data: Option<&TheaterData>,
    asset_manager: Option<&crate::assets::asset_manager::AssetManager>,
    terrain_rules: Option<&TerrainRules>,
    key: TileKey,
    warned_unknown_land_types: &mut HashSet<u8>,
) -> TileMetadata {
    let Some(td) = theater_data else {
        return TileMetadata::default();
    };
    let Some(asset_manager) = asset_manager else {
        return metadata_from_set_name(
            td.lookup
                .tileset_index(key.tile_id)
                .and_then(|idx| td.lookup.set_name(idx)),
            td.lookup.tileset_index(key.tile_id),
        );
    };
    let tileset_index = td.lookup.tileset_index(key.tile_id);
    let set_name = tileset_index.and_then(|idx| td.lookup.set_name(idx));
    let mut metadata = metadata_from_set_name(set_name, tileset_index);

    let Some(filename) = td.lookup.filename_for_variant(key.tile_id, key.variant) else {
        mark_invalid_subtile_metadata(&mut metadata, terrain_rules);
        return metadata;
    };
    let Some(bytes) = asset_manager.get(filename) else {
        mark_invalid_subtile_metadata(&mut metadata, terrain_rules);
        return metadata;
    };
    let Ok(tmp) = TmpFile::from_bytes(&bytes) else {
        mark_invalid_subtile_metadata(&mut metadata, terrain_rules);
        return metadata;
    };
    metadata.tmp_file_valid = true;
    merge_tmp_file_metadata(
        &mut metadata,
        &tmp,
        key.sub_tile,
        terrain_rules,
        warned_unknown_land_types,
    );
    metadata
}

pub(super) fn merge_tmp_file_metadata(
    metadata: &mut TileMetadata,
    tmp: &TmpFile,
    sub_tile: u8,
    terrain_rules: Option<&TerrainRules>,
    warned_unknown_land_types: &mut HashSet<u8>,
) {
    metadata.template_width_cells = tmp.template_width;
    metadata.template_height_cells = tmp.template_height;
    // CellClass::RecalcAttributes validates the requested entry before asking
    // GetSubtileLandType to decode it. The latter's modulo is therefore not an
    // active fallback for a sparse or positive-OOB Cell subtile.
    let entry_count = tmp
        .template_width
        .checked_mul(tmp.template_height)
        .and_then(|count| usize::try_from(count).ok());
    let tile = entry_count
        .filter(|&count| usize::from(sub_tile) < count)
        .and_then(|_| tmp.tiles.get(usize::from(sub_tile)))
        .and_then(|tile| tile.as_ref());
    metadata.subtile_entry_valid = Some(tile.is_some());
    let relative_extra_y = tile.map_or(0, |tile| tile.relative_extra_y);
    match height_in_pixels_from_tmp(tmp.tile_height, relative_extra_y) {
        Some(height_in_pixels) => metadata.height_in_pixels = height_in_pixels,
        None => log::warn!(
            "ResolvedTerrain: TMP effective height overflow (header {}, relative extra-Y {})",
            tmp.tile_height,
            relative_extra_y,
        ),
    }
    let Some(tile) = tile else {
        mark_invalid_subtile_metadata(metadata, terrain_rules);
        return;
    };
    merge_tmp_metadata(metadata, tile);
    apply_land_type_semantics(metadata, terrain_rules, warned_unknown_land_types);
}

fn mark_invalid_subtile_metadata(
    metadata: &mut TileMetadata,
    terrain_rules: Option<&TerrainRules>,
) {
    metadata.subtile_entry_valid = Some(false);
    metadata.has_tmp_metadata = false;
    metadata.raw_land_type = 0;
    metadata.slope_type = 0;
    metadata.template_height = 0;
    metadata.has_ramp = false;
    apply_canonical_land_to_metadata(metadata, LandType::Clear, None, false);
    metadata.build_blocked = false;
    if let Some(clear) =
        terrain_rules.and_then(|rules| rules.semantics_for_land_type(LandType::Clear.as_index()))
    {
        metadata.terrain_class = clear.terrain_class;
        metadata.speed_costs = clear.speed_costs;
        metadata.is_water = clear.water;
        metadata.is_cliff_like = clear.cliff_like;
        metadata.is_rough = clear.rough;
        metadata.is_road = clear.road;
        metadata.ground_blocked = clear.ground_blocked;
        metadata.build_blocked = !clear.buildable;
    }
}

pub(super) fn height_in_pixels_from_tmp(tile_height: u32, relative_extra_y: i32) -> Option<i8> {
    let header_height = i32::try_from(tile_height).ok()?;
    let effective_height = header_height.checked_sub(relative_extra_y)?;
    let numerator = effective_height.checked_sub(30)?;
    Some((numerator / 15) as i8)
}

pub(super) fn metadata_from_set_name(
    set_name: Option<&str>,
    tileset_index: Option<u16>,
) -> TileMetadata {
    let lower = set_name.unwrap_or("").to_ascii_lowercase();
    let is_water = lower.contains("water");
    let is_cliff_like = lower.contains("cliff") || lower.contains("rock");
    let is_rough = lower.contains("rough");
    let is_road = lower.contains("road") || lower.contains("pavement") || lower.contains("pave");
    let land_type = if is_water {
        crate::rules::terrain_rules::LandType::Water.as_index()
    } else if is_road {
        crate::rules::terrain_rules::LandType::Road.as_index()
    } else if is_rough {
        crate::rules::terrain_rules::LandType::Rough.as_index()
    } else if is_cliff_like {
        crate::rules::terrain_rules::LandType::Rock.as_index()
    } else {
        crate::rules::terrain_rules::LandType::Clear.as_index()
    };
    let terrain_class = if is_water {
        TerrainClass::Water
    } else if lower.contains("cliff") {
        TerrainClass::Cliff
    } else if lower.contains("rock") {
        TerrainClass::Rock
    } else if is_road {
        TerrainClass::Road
    } else if is_rough {
        TerrainClass::Rough
    } else if !lower.is_empty() {
        TerrainClass::Clear
    } else {
        TerrainClass::Unknown
    };

    TileMetadata {
        tileset_index,
        land_type,
        yr_cell_land_type: land_type,
        terrain_class,
        is_water,
        is_cliff_like,
        is_rough,
        is_road,
        ground_blocked: is_water || is_cliff_like,
        build_blocked: is_water || is_cliff_like,
        ..TileMetadata::default()
    }
}

pub(super) fn merge_tmp_metadata(metadata: &mut TileMetadata, tile: &TmpTile) {
    metadata.subtile_entry_valid = Some(true);
    metadata.raw_land_type = tile.terrain_type;
    metadata.yr_cell_land_type = yr_cell_land_type_from_tmp(tile.terrain_type);
    metadata.land_type =
        crate::rules::terrain_rules::tmp_terrain_to_land_type(tile.terrain_type).as_index();
    metadata.slope_type = tile.ramp_type;
    metadata.template_height = tile.height;
    metadata.render_offset_x = tile.offset_x;
    metadata.render_offset_y = tile.offset_y;
    metadata.has_ramp = tile.ramp_type != 0;
    metadata.has_tmp_metadata = true;
    metadata.radar_left = tile.radar_left;
    metadata.radar_right = tile.radar_right;
    metadata.has_damaged_data = tile.has_damaged_data;
}

pub(super) fn yr_cell_land_type_from_tmp(tmp_terrain_type: u8) -> u8 {
    if tmp_terrain_type == 5 {
        YR_CELL_LAND_TUNNEL
    } else {
        crate::rules::terrain_rules::tmp_terrain_to_land_type(tmp_terrain_type).as_index()
    }
}

/// Maps TMP ramp_type byte to canonical direction.
/// Values from TS++ TIBSUN_DEFINES.H. Tilt matrix angles:
/// 270 deg=W, 180 deg=N, 90 deg=E, 0 deg=S for slope types 1-4.
pub(super) fn canonical_ramp_from_slope_type(slope_type: u8) -> Option<RampDirection> {
    match slope_type {
        1 => Some(RampDirection::West),
        2 => Some(RampDirection::North),
        3 => Some(RampDirection::East),
        4 => Some(RampDirection::South),
        _ => None,
    }
}

pub(super) fn apply_land_type_semantics(
    metadata: &mut TileMetadata,
    terrain_rules: Option<&TerrainRules>,
    warned_unknown_land_types: &mut HashSet<u8>,
) {
    let Some(terrain_rules) = terrain_rules else {
        return;
    };
    if !metadata.has_tmp_metadata {
        return;
    }
    // Rules rows use the canonical LandType index. The raw TMP byte remains
    // diagnostic context only after the fixed TMP-to-land conversion has run.
    let Some(semantics) = terrain_rules
        .semantics_for_land_type(metadata.land_type)
        .copied()
    else {
        if warned_unknown_land_types.insert(metadata.raw_land_type) {
            log::warn!(
                "No terrain-rules row for canonical LandType {} converted from TMP byte {}; \
                 falling back to tileset-name heuristics",
                metadata.land_type,
                metadata.raw_land_type,
            );
        }
        return;
    };

    metadata.terrain_class = semantics.terrain_class;
    metadata.speed_costs = semantics.speed_costs;
    metadata.is_water = semantics.water;
    metadata.is_cliff_like = semantics.cliff_like;
    metadata.is_rough = semantics.rough;
    metadata.is_road = semantics.road;
    metadata.ground_blocked = semantics.ground_blocked;
    metadata.build_blocked = !semantics.buildable;
}

/// Replace every land-derived attribute of a cell with the overlay's `Land=` row.
///
/// Overlay land override: `CellClass__RecalcAttributes` @ `0x0047D2B0`. Its
/// entry branch loads `OverlayTypeClass+0x298` into `Cell->LandType` and, when
/// that land is Wall(4)/Railroad(9) or `+0x2AC` (`NoUseTileLandType`) is set,
/// runs the LAT fixup and zone recompute and **returns** — the tile's own
/// subtile land type is never consulted. LandType is the only land attribute
/// gamemd stores, so nothing can survive the swap; passability is re-derived
/// from it. `ground_blocked` is VERA's cache of that derivation and must be
/// replaced here too, or a low bridge (`Land=Road`, `NoUseTileLandType=yes`)
/// over water keeps the water tile's block and rejects every ground unit.
pub(super) fn apply_canonical_land_to_metadata(
    metadata: &mut TileMetadata,
    land: LandType,
    speed_costs: Option<SpeedCostProfile>,
    ground_blocked: bool,
) {
    metadata.land_type = land.as_index();
    metadata.yr_cell_land_type = land.as_index();
    metadata.terrain_class = land.terrain_class();
    metadata.speed_costs = speed_costs.unwrap_or_default();
    metadata.is_water = land.is_water();
    metadata.is_cliff_like = land.is_cliff_like();
    metadata.is_rough = land.is_rough();
    metadata.is_road = land.is_road();
    metadata.ground_blocked = ground_blocked;
}

pub(super) fn classify_overlay_effects(
    overlays: Option<&Vec<&OverlayEntry>>,
    overlay_registry: Option<&OverlayTypeRegistry>,
    level: u8,
    slope_type: u8,
) -> OverlayEffects {
    let mut result = OverlayEffects::default();
    let Some(entries) = overlays else {
        return result;
    };
    for overlay in entries {
        let name = overlay_registry
            .and_then(|reg| reg.name(overlay.overlay_id))
            .unwrap_or("");
        // Bridge overlays identified by hardcoded index, matching original engine.
        let is_bridge = crate::rules::overlay_types::is_bridge_overlay_index(overlay.overlay_id);

        let flags = overlay_registry.and_then(|reg| reg.flags(overlay.overlay_id));
        if let Some(flags) = flags {
            // The gate on the early authoritative-land branch, evaluated before
            // the resource-overlay removal so it matches the engine's order.
            if uses_early_recalc_land_branch(flags) {
                result.claims_cell_attributes = true;
            }
            // The early branch copies Land before it clears a sloped resource.
            // The overlay pointer and its zone effects disappear, but this
            // invocation retains that already-copied current Cell land.
            if let Some(land) = retained_overlay_land(flags, slope_type) {
                result.effective_land = Some(land);
                result.effective_land_speed_costs = flags.land_speed_costs;
                result.effective_land_ground_blocked = flags.land_ground_blocked;
            }
            if clears_tiberium_on_slope(flags, slope_type) {
                continue;
            }
            result.overlay_zone_type = merge_overlay_zone_type(
                result.overlay_zone_type,
                overlay_reduced_zone_type(Some(flags)),
            );
            result.overlay_blocks = matches!(
                result.overlay_zone_type,
                Some(zone_class::WALL) | Some(zone_class::IMPASSABLE)
            );
        }
        if is_bridge && result.bridge_layer.is_none() {
            result.has_bridge_deck = true;
            // Direction determined by index: 24/237=EW, 25/238=NS, rest=Low.
            let direction = match overlay.overlay_id {
                24 | 237 => BridgeDirection::EastWest,
                25 | 238 => BridgeDirection::NorthSouth,
                _ => BridgeDirection::Low,
            };
            // High bridges: deck 4 levels above ground (HighBridgeHeight=4).
            // Low bridges: deck at ground level (no elevation change).
            let deck_level = match direction {
                BridgeDirection::EastWest | BridgeDirection::NorthSouth => level.saturating_add(4),
                BridgeDirection::Low => level,
            };
            if direction == BridgeDirection::Low {
                result.is_low_bridge = true;
            }
            result.bridge_layer = Some(BridgeLayer {
                overlay_id: overlay.overlay_id,
                overlay_name: name.to_string(),
                deck_level,
                direction,
            });
        }
    }
    result
}

fn merge_overlay_zone_type(current: Option<u8>, candidate: Option<u8>) -> Option<u8> {
    fn priority(zone_type: u8) -> u8 {
        match zone_type {
            zone_class::CRUSHABLE => 0,
            zone_class::WALL => 1,
            zone_class::IMPASSABLE => 2,
            zone_class::GROUND => 3,
            _ => u8::MAX,
        }
    }

    match (current, candidate) {
        (None, candidate) => candidate,
        (current, None) => current,
        (Some(current), Some(candidate)) => Some(if priority(candidate) < priority(current) {
            candidate
        } else {
            current
        }),
    }
}

pub(super) fn wheel_speed_at_or_below_one_percent(wheel: Option<u8>) -> bool {
    wheel.is_some_and(|speed| speed <= 1)
}

/// Reproduce the active map-load order: initialize every allocated Size-diamond
/// CellClass, then let IsoMapPack records overwrite those cells in stream order.
fn selector_free_map_cells(map: &MapFile, lookup: Option<&TilesetLookup>) -> Vec<MapCell> {
    let translate_raw_pack_cells = !map.iso_map_pack_lookups.is_empty();
    map.cells
        .iter()
        .cloned()
        .map(|mut cell| {
            if translate_raw_pack_cells && let Some(lookup) = lookup {
                cell.tile_index = lookup.translate_legacy_map_tile_index(cell.tile_index);
            }
            cell
        })
        .collect()
}

pub(super) fn materialize_map_load_cells(
    map: &MapFile,
    lookup: Option<&TilesetLookup>,
    selector: &mut TileVariantSelectionContext<'_, '_>,
    scenario_fill_ranged: &mut dyn FnMut(u32, u32) -> u32,
    shared_cell_dummy: &SharedCellDummy,
) -> Vec<MapCell> {
    let n = map.header.width;
    let m = map.header.height;
    let Some(max_coord) = n.checked_add(m) else {
        return Vec::new();
    };
    // The native owner is a fixed 512x512 CellClass lookup. Retail maps fit
    // this boundary; reject malformed dimensions instead of iterating an
    // unbounded signed-bit-pattern value from the INI.
    if n == 0 || m == 0 || max_coord > 512 {
        return Vec::new();
    }

    let is_water = map
        .header
        .fill
        .trim_matches(|ch: char| ch <= ' ')
        .eq_ignore_ascii_case("Water");
    let expected_count = m
        .checked_mul(n.saturating_mul(2).saturating_sub(1))
        .and_then(|count| usize::try_from(count).ok())
        .unwrap_or(0);
    let mut cells = Vec::with_capacity(expected_count);
    let mut index_by_coord = HashMap::with_capacity(expected_count);
    let last_sum = n + m * 2;

    // This sum-major form is the native iterator's anti-diagonal order: each
    // diagonal advances x and retreats y, beginning at (1, Size.Width).
    for sum in (n + 1)..=last_sum {
        for rx in 0..=sum {
            let ry = sum - rx;
            if rx.abs_diff(ry) >= n {
                continue;
            }
            let rx = u16::try_from(rx).expect("validated Size diamond x fits u16");
            let ry = u16::try_from(ry).expect("validated Size diamond y fits u16");
            let index = cells.len();
            cells.push(MapCell {
                rx,
                ry,
                tile_index: selector.draw_fill_tile_index(is_water, scenario_fill_ranged),
                sub_tile: 0,
                z: 0u8.wrapping_add(map.header.level as u8),
            });
            index_by_coord.insert((rx, ry), index);
        }
    }
    debug_assert_eq!(cells.len(), expected_count);

    // gamemd.exe IsoMapPack5 decoder @ 0x0056BAC0: every OOB or null-slot
    // lookup stamps its exact raw header into the fixed dummy's CellStruct
    // before consuming (but not applying) the payload. Valid lookups never
    // touch the dummy, so replaying only misses here preserves stream order's
    // last writer while keeping the parser pure and payload authority real-only.
    if map.iso_map_pack_lookups.is_empty() {
        // Synthetic/RMG constructors have no decoder trace. Retain their
        // established manual-cell behavior while giving canonical misses the
        // same dummy-coordinate side effect as a native lookup.
        for explicit in &map.cells {
            if !index_by_coord.contains_key(&(explicit.rx, explicit.ry)) {
                shared_cell_dummy.stamp_coord(i32::from(explicit.rx), i32::from(explicit.ry));
            }
        }
    } else {
        for lookup in &map.iso_map_pack_lookups {
            let misses_real_slot = lookup
                .canonical
                .is_none_or(|coord| !index_by_coord.contains_key(&coord));
            if misses_real_slot {
                shared_cell_dummy.stamp_coord(i32::from(lookup.raw_x), i32::from(lookup.raw_y));
            }
        }
    }

    let translate_raw_pack_cells = !map.iso_map_pack_lookups.is_empty();
    for explicit in &map.cells {
        let Some(&index) = index_by_coord.get(&(explicit.rx, explicit.ry)) else {
            continue;
        };
        let mut materialized = explicit.clone();
        if translate_raw_pack_cells && let Some(lookup) = lookup {
            // Retail provenance: Pack5 callsite 0x0056BB68 reaches
            // `CalculateLegacyMapTileIndex @ 0x00544E30` only after the
            // destination CellClass was accepted, and before Cell+0x38 becomes
            // authoritative for variant/LAT/Recalc consumers. Fill, RMG, and
            // manual/runtime actual IDs never cross this seam.
            materialized.tile_index =
                lookup.translate_legacy_map_tile_index(materialized.tile_index);
        }
        cells[index] = materialized;
    }
    cells
}
