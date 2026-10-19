//! `CellClass::RecalcAttributes` for authored loading and runtime bridge
//! reconstruction, with its load-local inputs and effect callbacks.

use super::build::{
    TileMetadata, apply_load_land_to_cell, apply_pristine_load_metadata, cached_tile_metadata,
    canonical_ramp_from_slope_type, cliff_back_normal_reclass_applies, is_wood_bridge_repair_tile,
    restore_load_base_land,
};
use super::tubes::auto_tube_direction_for_tile;
use super::*;

/// Non-Clone input adapter for `CellClass::RecalcAttributes`. Authored loading
/// owns its lazy asset cache and animation latch; the admitted runtime bridge
/// path borrows immutable resident inputs and supplies current playfield bounds.
///
/// The metadata/cache inputs are retained across calls so every Mark write and
/// both whole-cell sweeps observe the mutations made by earlier calls. The
/// animation latch is the load-local analogue of `CellClass+0x140 & 0x20000`;
/// it is deliberately not stored in the cloneable final terrain projection.
pub(crate) struct LoadCellRecalcState<'a> {
    theater_data: Option<&'a TheaterData>,
    asset_manager: Option<&'a crate::assets::asset_manager::AssetManager>,
    resident_bridge: Option<&'a BridgeRecalcCatalog>,
    terrain_rules: Option<&'a TerrainRules>,
    overlay_types: &'a OverlayTypeRegistry,
    playfield: Option<PlayfieldBounds>,
    pub(super) lat_config: Option<lat::LatConfig>,
    pub(super) slope_config: Option<lat::SlopeFixupConfig>,
    pub(super) cliff_back_impassability: u8,
    metadata_cache: HashMap<TileKey, TileMetadata>,
    pub(super) terrain_anim_latched: Vec<bool>,
    pixel_conversion_bounds: PixelConversionBounds,
}

impl<'a> LoadCellRecalcState<'a> {
    pub(crate) fn for_authored_load(
        map: &MapFile,
        theater_data: &'a TheaterData,
        asset_manager: &'a crate::assets::asset_manager::AssetManager,
        terrain_rules: &'a TerrainRules,
        overlay_types: &'a OverlayTypeRegistry,
        lat_enabled: bool,
        cliff_back_impassability: u8,
        cell_count: usize,
        pixel_conversion_bounds: PixelConversionBounds,
    ) -> Self {
        let ids = crate::map::rmg::tiles::TileIds::from_keys(&theater_data.rmg_tiles);
        let lat_config = lat_enabled.then(|| lat::LatConfig::from_tile_ids(&ids));
        let slope_config = lat_enabled.then(|| lat::SlopeFixupConfig::from_tile_ids(&ids));
        Self {
            theater_data: Some(theater_data),
            asset_manager: Some(asset_manager),
            resident_bridge: None,
            terrain_rules: Some(terrain_rules),
            overlay_types,
            playfield: Some(PlayfieldBounds::from_map_header(&map.header)),
            lat_config,
            slope_config,
            cliff_back_impassability,
            metadata_cache: HashMap::new(),
            terrain_anim_latched: vec![false; cell_count],
            pixel_conversion_bounds,
        }
    }

    /// Explicitly synthetic fallback for focused map-owner tests. Production
    /// authored loading must use `for_authored_load` so missing theater/TMP
    /// authority cannot silently become heuristic metadata.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn synthetic(overlay_types: &'a OverlayTypeRegistry, cell_count: usize) -> Self {
        Self {
            theater_data: None,
            asset_manager: None,
            resident_bridge: None,
            terrain_rules: None,
            overlay_types,
            playfield: None,
            lat_config: None,
            slope_config: None,
            cliff_back_impassability: 0,
            metadata_cache: HashMap::new(),
            terrain_anim_latched: vec![false; cell_count],
            pixel_conversion_bounds: PixelConversionBounds::default(),
        }
    }

    /// Clear exactly the receiver latch immediately before that cell's final
    /// load Recalc. Native `InitCellAttributes(0)` interleaves this write with
    /// the anti-diagonal sweep; clearing the whole vector ahead of the sweep
    /// would expose future cells too early.
    pub(crate) fn clear_terrain_anim_latch(&mut self, index: usize) -> bool {
        let latched = self
            .terrain_anim_latched
            .get_mut(index)
            .expect("load Recalc latch shape must match the real terrain grid");
        let was_latched = *latched;
        *latched = false;
        was_latched
    }

    fn terrain_anim_is_latched(&self, index: usize) -> bool {
        if self.resident_bridge.is_some() {
            // Resident admission rejects every declared source attachment.
            // No per-call false latch is used to authorize an Anim constructor.
            return false;
        }
        *self
            .terrain_anim_latched
            .get(index)
            .expect("load Recalc latch shape must match the real terrain grid")
    }

    fn latch_terrain_anim(&mut self, index: usize) {
        *self
            .terrain_anim_latched
            .get_mut(index)
            .expect("load Recalc latch shape must match the real terrain grid") = true;
    }

    fn registered_tile_metadata<E>(
        &mut self,
        tile_index: i32,
        sub_tile: u8,
        synthetic_cell: &ResolvedTerrainCell,
    ) -> Result<Option<TileMetadata>, LoadCellRecalcError<E>> {
        if let Some(catalog) = self.resident_bridge {
            return catalog
                .metadata(tile_index, sub_tile)
                .map_err(LoadCellRecalcError::ResidentInput);
        }
        if tile_index < 0 || tile_index == 0xFFFF {
            return Ok(None);
        }
        if let Some(theater) = self.theater_data {
            if tile_index as usize >= theater.lookup.len() {
                return Ok(None);
            }
            let tile_id = tile_index as u16;
            return Ok(Some(cached_tile_metadata(
                &mut self.metadata_cache,
                self.theater_data,
                self.asset_manager,
                self.terrain_rules,
                TileKey {
                    tile_id,
                    sub_tile,
                    variant: 0,
                },
            )));
        }

        #[cfg(any(test, feature = "test-support"))]
        {
            Ok(Some(TileMetadata::from_resolved_cell(synthetic_cell)))
        }
        #[cfg(not(any(test, feature = "test-support")))]
        {
            let _ = synthetic_cell;
            unreachable!("production load Recalc requires theater/TMP authority")
        }
    }

    pub(super) fn for_resident_bridge(
        catalog: &'a BridgeRecalcCatalog,
        overlay_types: &'a OverlayTypeRegistry,
        bounds: Option<PlayfieldBounds>,
    ) -> Self {
        Self {
            theater_data: None,
            asset_manager: None,
            resident_bridge: Some(catalog),
            terrain_rules: Some(&catalog.terrain_rules),
            overlay_types,
            playfield: bounds,
            lat_config: catalog.lat_config.clone(),
            slope_config: catalog.slope_config,
            cliff_back_impassability: catalog.cliff_back_impassability,
            metadata_cache: HashMap::new(),
            terrain_anim_latched: Vec::new(),
            pixel_conversion_bounds: PixelConversionBounds::default(),
        }
    }

    fn current_permissions(&self, tile: i32) -> Option<(bool, bool)> {
        self.resident_bridge
            .map(|catalog| catalog.current_permissions(tile))
            .or_else(|| {
                self.theater_data
                    .map(|theater| current_tile_permissions(&theater.lookup, tile))
            })
    }

    fn automatic_tube_direction(&self, tile: i32) -> Option<u8> {
        if let Some(catalog) = self.resident_bridge {
            catalog.automatic_tube_direction(tile)
        } else {
            auto_tube_direction_for_tile(tile, self.theater_data)
        }
    }

    fn wood_bridge_repair_tile(&self, tile: i32) -> bool {
        self.resident_bridge.map_or_else(
            || is_wood_bridge_repair_tile(self.theater_data, tile),
            |catalog| catalog.is_wood_bridge_repair_tile(tile),
        )
    }

    fn clear_land_metadata(&self) -> TileMetadata {
        let clear = land_row(self.terrain_rules, LandType::Clear);
        TileMetadata {
            land_type: LandType::Clear.as_index(),
            speed_costs: clear.speed_costs,
            ground_blocked: clear.ground_blocked,
            build_blocked: !clear.buildable,
            ..TileMetadata::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AutomaticTubeRequest {
    pub(crate) cell: (u16, u16),
    pub(crate) direction: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutomaticTubeAllocation {
    /// Native's failed allocation; only test hosts answer it.
    #[cfg(any(test, feature = "test-support"))]
    AllocationNull,
    Allocated {
        native_unique_id: i32,
        /// Deterministic test seam for the post-ID vector-growth failure. The
        /// production host returns true and the map owner still uses
        /// `try_reserve` before publishing the registry entry.
        registry_append_allowed: bool,
    },
}

pub(crate) trait LoadCellRecalcEffects {
    type Error;

    /// Model the outer allocation and shared native-ID assignment. Registry
    /// append and cell-index publication remain map-owned and occur before the
    /// later terrain Anim callback.
    fn allocate_automatic_tube(
        &mut self,
        request: AutomaticTubeRequest,
    ) -> Result<AutomaticTubeAllocation, Self::Error>;

    /// Complete the terrain-attached Anim constructor/Middle side effects
    /// synchronously. The map owner sets its cell latch only after this call
    /// succeeds.
    fn construct_terrain_attached_anim(
        &mut self,
        request: &TerrainTileAnimation,
    ) -> Result<(), Self::Error>;
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum LoadCellRecalcError<E> {
    #[error("runtime Recalc has no resident bridge inputs")]
    MissingResidentInputs,
    #[error(transparent)]
    ResidentInput(BridgeRecalcCatalogError),
    #[error("load Recalc received non-native overlay identity {identity}")]
    MalformedOverlayIdentity { identity: i32 },
    #[error("load Recalc could not resolve OverlayType index {overlay_id}")]
    MissingOverlayType { overlay_id: u8 },
    #[error("load Recalc cell index {index} is outside the live terrain grid")]
    CellIndexOutOfBounds { index: usize },
    #[error("terrain-attached animation construction failed during load Recalc")]
    Effect(E),
}

#[derive(Debug)]
pub(crate) struct LoadCellRecalcOutcome {
    pub(crate) finalized: FinalizedOverlayCell,
    pub(crate) zone_before: u8,
    pub(crate) zone_after: u8,
}

impl ResolvedTerrainGrid {
    /// Execute one live authored-load `CellClass::RecalcAttributes(-1)`.
    ///
    /// gamemd-derived: `CellClass::RecalcAttributes @ 0x0047D2B0`. The shared
    /// dummy is filtered by the caller before this method; every real call
    /// consumes the current identity, mutates the one live terrain cell, sees
    /// earlier neighbor mutations, and may synchronously construct exactly one
    /// terrain-attached Anim before latching the cell.
    pub(crate) fn recalc_authored_load_cell<E: LoadCellRecalcEffects>(
        &mut self,
        state: &mut LoadCellRecalcState<'_>,
        index: usize,
        overlay: FinalizedOverlayCell,
        effects: &mut E,
    ) -> Result<LoadCellRecalcOutcome, LoadCellRecalcError<E::Error>> {
        self.recalc_cell_attributes(state, index, overlay, -1, effects)
    }

    /// Shared47D2B0 entry: only the valid normal-TMP branch consumes the
    /// level override, after Tube construction and before dimensions/Anim.
    /// Evidence: tools/spatial_oracle/terrain_recalc.
    pub(crate) fn recalc_cell_attributes<E: LoadCellRecalcEffects>(
        &mut self,
        state: &mut LoadCellRecalcState<'_>,
        index: usize,
        overlay: FinalizedOverlayCell,
        level_override: i32,
        effects: &mut E,
    ) -> Result<LoadCellRecalcOutcome, LoadCellRecalcError<E::Error>> {
        let Some(snapshot) = self.cells.get(index).cloned() else {
            return Err(LoadCellRecalcError::CellIndexOutOfBounds { index });
        };
        let zone_before = snapshot.zone_type;
        let source_overlay_id = match overlay.identity() {
            NO_OVERLAY_IDENTITY => None,
            identity @ 0..=254 => Some(identity as u8),
            identity => {
                return Err(LoadCellRecalcError::MalformedOverlayIdentity { identity });
            }
        };
        let source_flags = source_overlay_id
            .map(|overlay_id| {
                state
                    .overlay_types
                    .flags(overlay_id)
                    .ok_or(LoadCellRecalcError::MissingOverlayType { overlay_id })
            })
            .transpose()?;
        let early_overlay_branch = source_flags.is_some_and(uses_early_recalc_land_branch);
        let mut finalized = overlay;
        let mut current_land_ground_blocked;
        let mut current_cliff_eligible;
        let mut base_cliff_eligible = cliff_back_normal_reclass_applies(snapshot.base_land_type);
        let run_lat;
        let mut anim_request = None;
        let mut animation_tile_id = None;
        let mut animation_sub_tile = 0;
        let mut valid_entry = false;
        let mut sparse_entry = false;
        let mut retained_height_in_pixels = snapshot.height_in_pixels;

        if early_overlay_branch {
            let flags = source_flags.expect("early branch has OverlayType flags");
            apply_load_land_to_cell(
                &mut self.cells[index],
                flags.land,
                flags.land_speed_costs,
                flags.land_ground_blocked,
            );
            current_land_ground_blocked = flags.land_ground_blocked;
            // `CellClass::RecalcAttributes @ 0x0047D2B0` re-reads the current
            // pristine TMP slope before the early overlay branch validates a
            // sloped resource. A registered sparse/missing TMP yields flat 0;
            // an unregistered tile has no proved replacement and retains the
            // cached receiver byte.
            let refreshed_slope = state
                .registered_tile_metadata(
                    snapshot.final_tile_index,
                    snapshot.final_sub_tile,
                    &snapshot,
                )?
                .map_or(snapshot.slope_type, |metadata| metadata.slope_type);
            {
                let cell = &mut self.cells[index];
                cell.slope_type = refreshed_slope;
                cell.has_ramp = refreshed_slope != 0;
                cell.canonical_ramp = canonical_ramp_from_slope_type(refreshed_slope);
            }
            if clears_tiberium_on_slope(flags, refreshed_slope) {
                finalized = FinalizedOverlayCell::default();
            }
            current_cliff_eligible = true;
            if state.cliff_back_impassability != 0 {
                let behind_cliff = self.authored_load_cell_is_behind_cliff(index);
                if behind_cliff
                    && state.cliff_back_impassability == 2
                    && let Some(ground_blocked) = self.apply_authored_load_cliff_back_rock(
                        state,
                        index,
                        base_cliff_eligible,
                        current_cliff_eligible,
                    )
                {
                    current_land_ground_blocked = ground_blocked;
                }
            }
            run_lat = true;
        } else {
            let current_tile = snapshot.final_tile_index;
            let current_sub_tile = snapshot.final_sub_tile;
            let registered =
                state.registered_tile_metadata(current_tile, current_sub_tile, &snapshot)?;
            sparse_entry = registered
                .as_ref()
                .is_some_and(|metadata| metadata.subtile_entry_valid == Some(false));
            valid_entry = registered.is_some() && !sparse_entry;
            let metadata = if valid_entry {
                registered.expect("valid registered tile metadata")
            } else {
                state.clear_land_metadata()
            };
            let tile_id = u16::try_from(current_tile).ok();
            if valid_entry {
                // `RecalcAttributes @ 0x0047D2B0` retains the registered
                // pristine tile receiver for the later AnimType/AttachesTo
                // checks even though LAT may replace Cell+0x38 in between.
                animation_tile_id = tile_id;
                animation_sub_tile = current_sub_tile;
                retained_height_in_pixels = metadata.height_in_pixels;
            }
            let permissions = state
                .current_permissions(current_tile)
                .unwrap_or((false, false));
            let accepts_smudge = valid_entry && permissions.0;
            let allows_tiberium = valid_entry && permissions.1;
            {
                let cell = &mut self.cells[index];
                if !valid_entry {
                    cell.final_tile_index = 0xFFFF;
                    if sparse_entry {
                        cell.final_sub_tile = 0;
                    }
                }
                apply_pristine_load_metadata(cell, &metadata, accepts_smudge, allows_tiberium);
                // Invalid/sparse paths preserve11D; the valid path updates it
                // only after its possible automatic Tube constructor.
                cell.height_in_pixels = snapshot.height_in_pixels;
                restore_load_base_land(cell);
            }
            base_cliff_eligible = if sparse_entry {
                self.cells[index].base_land_type == LandType::Clear.as_index()
            } else {
                cliff_back_normal_reclass_applies(self.cells[index].base_land_type)
            };
            current_cliff_eligible = if sparse_entry {
                self.cells[index].land_type == LandType::Clear.as_index()
            } else {
                cliff_back_normal_reclass_applies(self.cells[index].land_type)
            };
            current_land_ground_blocked = self.cells[index].base_ground_walk_blocked;
            run_lat = valid_entry;
        }

        if run_lat {
            self.apply_authored_load_lat_slope(state, index);
        }

        // These are cached current-tile queries, unlike retained native Cell
        // attributes. CanPlaceTiberium4839C0 and smudge6B5F80 resolve live+38.
        if let Some(permissions) = state.current_permissions(self.cells[index].final_tile_index) {
            let cell = &mut self.cells[index];
            (cell.accepts_smudge, cell.allows_tiberium) = permissions;
        }

        if !early_overlay_branch {
            // 47D83C/47D8A3 and47D967 use retained EBP after47CA80.
            // LAT changes+38 and ensures final TMP residency; it does not
            // replace the pristine land/slope/dimensions receiver. The Tube
            // predicate separately reads the current tile identity.
            let refreshed_slope = self.cells[index].slope_type;
            if valid_entry
                && let Some(flags) = source_flags
                && clears_tiberium_on_slope(flags, refreshed_slope)
            {
                finalized = FinalizedOverlayCell::default();
            }
            let live_flags = finalized
                .overlay_id()
                .and_then(|overlay_id| state.overlay_types.flags(overlay_id));
            let retained_land = live_flags.and_then(|flags| {
                if valid_entry {
                    retained_overlay_land(flags, refreshed_slope)
                } else if sparse_entry {
                    // 47D5EF goes directly to the Clear fallback tail.
                    None
                } else {
                    // 47DB22 preserves an ordinary overlay's Land on the
                    // invalid/sentinel path. Early overlays exited above.
                    Some(flags.land)
                }
            });
            if let (Some(flags), Some(land)) = (live_flags, retained_land) {
                apply_load_land_to_cell(
                    &mut self.cells[index],
                    land,
                    flags.land_speed_costs,
                    flags.land_ground_blocked,
                );
                current_land_ground_blocked = flags.land_ground_blocked;
            } else {
                current_land_ground_blocked = self.cells[index].base_ground_walk_blocked;
            }
            base_cliff_eligible = if sparse_entry {
                self.cells[index].base_land_type == LandType::Clear.as_index()
            } else {
                cliff_back_normal_reclass_applies(self.cells[index].base_land_type)
            };
            current_cliff_eligible = if sparse_entry {
                self.cells[index].land_type == LandType::Clear.as_index()
            } else {
                cliff_back_normal_reclass_applies(self.cells[index].land_type)
            };

            if valid_entry {
                let cell = &self.cells[index];
                if cell.land_type == LandType::Tunnel.as_index()
                    && self.native_tube_indices[index]
                        .admits_automatic_construction(self.tube_facts.len())
                    && let Some(direction) = state.automatic_tube_direction(cell.final_tile_index)
                {
                    let request = AutomaticTubeRequest {
                        cell: (cell.rx, cell.ry),
                        direction,
                    };
                    match effects
                        .allocate_automatic_tube(request)
                        .map_err(LoadCellRecalcError::Effect)?
                    {
                        #[cfg(any(test, feature = "test-support"))]
                        AutomaticTubeAllocation::AllocationNull => {}
                        AutomaticTubeAllocation::Allocated {
                            native_unique_id,
                            registry_append_allowed,
                        } => {
                            let appended = registry_append_allowed
                                && self.tube_facts.try_reserve(1).is_ok()
                                && self.tube_native_ids.try_reserve(1).is_ok();
                            if appended {
                                let ordinal = self.tube_facts.len();
                                self.tube_facts.push(TubeFact::automatic_recalc_shell(
                                    request.cell,
                                    request.direction,
                                ));
                                self.tube_native_ids.push(Some(native_unique_id));
                                if request.cell != (0, 0) {
                                    let raw = NativeTubeCellIndex::from_registry_ordinal(ordinal);
                                    self.native_tube_indices[index] = raw;
                                    self.cells[index].tube_index =
                                        raw.validated_id(self.tube_facts.len());
                                }
                            } else if request.cell != (0, 0) {
                                self.native_tube_indices[index] = NativeTubeCellIndex::NONE;
                                self.cells[index].tube_index = None;
                            }
                        }
                    }
                }

                if level_override != -1 {
                    self.cells[index].level = level_override as u8;
                }
                self.cells[index].height_in_pixels = retained_height_in_pixels;

                if !state.terrain_anim_is_latched(index)
                    && live_flags.is_none_or(|flags| !uses_early_recalc_land_branch(flags))
                    && let (Some(theater), Some(tile_id)) = (state.theater_data, animation_tile_id)
                    && let Some(anim) = theater.lookup.tile_anim(tile_id)
                    && anim.attaches_to == i32::from(animation_sub_tile)
                {
                    let (offset_x, offset_y) = state
                        .pixel_conversion_bounds
                        .offset_to_leptons(anim.x_offset, anim.y_offset);
                    let cell = &self.cells[index];
                    anim_request = Some(TerrainTileAnimation {
                        rx: cell.rx,
                        ry: cell.ry,
                        anim_name: anim.anim_name.clone(),
                        world_x: offset_x
                            .wrapping_add(i32::from(cell.rx).wrapping_mul(LEPTONS_PER_CELL))
                            .wrapping_add(CELL_CENTRE_LEPTONS),
                        world_y: offset_y
                            .wrapping_add(i32::from(cell.ry).wrapping_mul(LEPTONS_PER_CELL))
                            .wrapping_add(CELL_CENTRE_LEPTONS),
                        world_z: i32::from(cell.level as i8)
                            .wrapping_mul(crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS),
                        z_adjust: anim.z_adjust,
                    });
                }
            }
        }

        if let Some(request) = anim_request.as_ref() {
            effects
                .construct_terrain_attached_anim(request)
                .map_err(LoadCellRecalcError::Effect)?;
            state.latch_terrain_anim(index);
        }

        if !early_overlay_branch && state.cliff_back_impassability != 0 {
            let behind_cliff = self.authored_load_cell_is_behind_cliff(index);
            if behind_cliff
                && state.cliff_back_impassability == 2
                && let Some(ground_blocked) = self.apply_authored_load_cliff_back_rock(
                    state,
                    index,
                    base_cliff_eligible,
                    current_cliff_eligible,
                )
            {
                current_land_ground_blocked = ground_blocked;
            }
        }

        self.finish_authored_load_cell_projection(
            state,
            index,
            finalized,
            current_land_ground_blocked,
        );
        let cell = &self.cells[index];
        Ok(LoadCellRecalcOutcome {
            finalized,
            zone_before,
            zone_after: cell.zone_type,
        })
    }

    pub(super) fn apply_authored_load_lat_slope(
        &mut self,
        state: &LoadCellRecalcState<'_>,
        index: usize,
    ) {
        let Some(cell) = self.cells.get(index) else {
            return;
        };
        let x = cell.rx as i16;
        let y = cell.ry as i16;
        let tile = cell.final_tile_index;
        let slope = cell.slope_type;
        let cardinal_coords = [
            (x, y.wrapping_sub(1)),
            (x.wrapping_add(1), y),
            (x, y.wrapping_add(1)),
            (x.wrapping_sub(1), y),
        ];
        let lat_tile = state.lat_config.as_ref().map_or(tile, |lat_config| {
            lat::lat_fixed_tile_live(tile, lat_config, || {
                cardinal_coords.map(|(nx, ny)| self.authored_load_neighbor_tile(nx, ny))
            })
        });
        let final_tile = state.slope_config.map_or(lat_tile, |slope_config| {
            lat::slope_fixed_tile_live(lat_tile, slope, slope_config, |slot| {
                let (nx, ny) = cardinal_coords[slot];
                self.authored_load_neighbor_slope(nx, ny)
            })
        });
        self.cells[index].final_tile_index = final_tile;
    }

    fn authored_load_neighbor_tile(&mut self, x: i16, y: i16) -> i32 {
        if let Some(index) = self.native_fixed_cell_index(x, y) {
            self.cells[index].final_tile_index
        } else {
            self.shared_cell_dummy
                .stamp_coord(i32::from(x), i32::from(y));
            0xFFFF
        }
    }

    fn authored_load_neighbor_slope(&mut self, x: i16, y: i16) -> u8 {
        if let Some(index) = self.native_fixed_cell_index(x, y) {
            self.cells[index].slope_type
        } else {
            self.shared_cell_dummy
                .stamp_coord(i32::from(x), i32::from(y));
            self.shared_cell_dummy.snapshot().slope_type
        }
    }

    pub(super) fn authored_load_cell_is_behind_cliff(&self, index: usize) -> bool {
        const OFFSETS: [(i16, i16); 6] = [(0, -1), (-1, 0), (2, 2), (1, 1), (-1, 1), (1, -1)];
        let cell = &self.cells[index];
        let x = cell.rx as i16;
        let y = cell.ry as i16;
        let threshold = i16::from(cell.level as i8).wrapping_add(4);
        for (dx, dy) in OFFSETS {
            let nx = x.wrapping_add(dx);
            let ny = y.wrapping_add(dy);
            let neighbor_level = if let Some(neighbor_index) = self.native_fixed_cell_index(nx, ny)
            {
                self.cells[neighbor_index].level as i8
            } else {
                self.shared_cell_dummy
                    .stamp_coord(i32::from(nx), i32::from(ny));
                self.shared_cell_dummy.snapshot().level
            };
            if i16::from(neighbor_level) >= threshold {
                return true;
            }
        }
        false
    }

    fn apply_authored_load_cliff_back_rock(
        &mut self,
        state: &LoadCellRecalcState<'_>,
        index: usize,
        apply_base: bool,
        apply_current: bool,
    ) -> Option<bool> {
        if state.cliff_back_impassability != 2 || (!apply_base && !apply_current) {
            return None;
        }
        // The Rock row, as a TMP Rock cell takes it.
        let rock = land_row(state.terrain_rules, LandType::Rock);
        let cell = &mut self.cells[index];
        if apply_base {
            cell.base_land_type = LandType::Rock.as_index();
            cell.base_speed_costs = rock.speed_costs;
            cell.base_ground_walk_blocked = rock.ground_blocked;
            cell.base_build_blocked = !rock.buildable;
        }
        if apply_current {
            cell.land_type = LandType::Rock.as_index();
            cell.speed_costs = rock.speed_costs;
        }
        apply_current.then_some(rock.ground_blocked)
    }

    fn finish_authored_load_cell_projection(
        &mut self,
        state: &LoadCellRecalcState<'_>,
        index: usize,
        finalized: FinalizedOverlayCell,
        land_ground_blocked: bool,
    ) {
        let overlay_id = finalized.overlay_id();
        let flags = overlay_id.and_then(|overlay_id| state.overlay_types.flags(overlay_id));
        let overlay_zone_type = overlay_reduced_zone_type(flags);
        let overlay_blocks = matches!(
            overlay_zone_type,
            Some(zone_class::WALL) | Some(zone_class::IMPASSABLE)
        );
        let overlay_name = overlay_id
            .and_then(|overlay_id| state.overlay_types.name(overlay_id))
            .unwrap_or("");
        let cell = &mut self.cells[index];
        cell.overlay_zone_type = overlay_zone_type;
        cell.overlay_blocks = overlay_blocks;
        cell.bridge_layer = overlay_id.and_then(|overlay_id| {
            crate::rules::overlay_types::is_bridge_overlay_index(overlay_id).then(|| {
                let direction = match overlay_id {
                    24 | 237 => BridgeDirection::EastWest,
                    25 | 238 => BridgeDirection::NorthSouth,
                    _ => BridgeDirection::Low,
                };
                BridgeLayer {
                    overlay_id,
                    overlay_name: overlay_name.to_string(),
                    deck_level: match direction {
                        BridgeDirection::Low => cell.level,
                        BridgeDirection::EastWest | BridgeDirection::NorthSouth => {
                            cell.level.saturating_add(4)
                        }
                    },
                    direction,
                }
            })
        });
        let identity_bridge = cell.bridge_layer.is_some();
        let structural_bridge = cell.bridge_facts.has_structural_bridge();
        cell.has_bridge_deck = identity_bridge || structural_bridge;
        cell.refresh_current_height_projection();
        cell.bridge_walkable =
            structural_bridge && !cell.terrain_object_blocks && !cell.overlay_blocks;
        cell.bridge_transition = cell.bridge_facts.has_transition_flag();
        cell.ground_walk_blocked =
            land_ground_blocked || cell.terrain_object_blocks || cell.overlay_blocks;
        cell.outside_playfield = state.playfield.is_some_and(|playfield| {
            !playfield.contains_height_aware_packed(
                i32::from(cell.rx),
                i32::from(cell.ry),
                cell.level as i8,
                cell.slope_type,
            )
        });
        cell.zone_type = recalc_zone_type(
            cell.outside_playfield,
            cell.overlay_zone_type,
            cell.land_type,
            cell.speed_costs.wheel,
            cell.terrain_object_occupation,
        );
        cell.bridge_facts.overlay_id = overlay_id;
        cell.bridge_facts.state_byte = finalized.state();
        cell.is_wood_bridge_repair_tile = state.wood_bridge_repair_tile(cell.final_tile_index);
    }

    /// Recompute the playfield-derived part of every live CellClass cache.
    ///
    /// `FUN_006E21E0`, reached by TriggerAction kind 0x28 from
    /// `TriggerAction__Execute @ 0x006DD8B0`, runs
    /// `CellClass::RecalcAttributes(-1) @ 0x0047D2B0` over all cells after the
    /// LocalSize writer. Rust stores the two affected derived facts directly:
    /// `outside_playfield` and the reduced `zone_type` that consumes it.
    /// Other RecalcAttributes inputs did not change, so rewriting them would
    /// manufacture unrelated runtime behavior.
    pub(crate) fn recalc_playfield_attributes(
        &mut self,
        bounds: PlayfieldBounds,
    ) -> Vec<(u16, u16)> {
        let (cells, native_allocated) = (&mut self.cells, &self.native_allocated);
        let mut refreshed = Vec::with_capacity(cells.len());
        for (index, cell) in cells.iter_mut().enumerate() {
            if native_allocated
                .as_ref()
                .is_some_and(|mask| !mask.get(index).copied().unwrap_or(false))
            {
                continue;
            }
            let outside = !bounds.contains_height_aware_packed(
                i32::from(cell.rx),
                i32::from(cell.ry),
                cell.level as i8,
                cell.slope_type,
            );
            cell.outside_playfield = outside;
            cell.zone_type = recalc_zone_type(
                outside,
                cell.overlay_zone_type,
                cell.land_type,
                cell.speed_costs.wheel,
                cell.terrain_object_occupation,
            );
            refreshed.push((cell.rx, cell.ry));
        }
        refreshed
    }
}
