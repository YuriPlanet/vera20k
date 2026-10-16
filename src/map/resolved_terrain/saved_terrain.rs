//! Saved materialized terrain values and their resident asset rebinding.
//!
//! MouseClass Load5BE150 restores Cell bodies before546DA0 visits them. Neither
//! that path nor this value boundary runs authored Fill/Recalc, Tube or Anim
//! constructors. Process identity and resident catalogs are supplied separately.

use super::build::load_native_tmp_draw_heights;
use super::cliffs::build_destroyable_cliff_catalog;
use super::*;
use crate::assets::asset_manager::AssetManager;
use crate::map::tile_variant_selector::TileVariantSelector;

/// The immutable materialized template owned by SimResources. Mutable saved
/// Cell replacements remain in the simulation's existing dynamic-cell owner.
/// All fields are private so deserialized values cross the map validator before
/// they become a grid. Asset bytes, caches and the process dummy are not saved.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SavedTerrainTemplate {
    width: u16,
    height: u16,
    cells: Vec<ResolvedTerrainCell>,
    native_allocated: Option<Vec<bool>>,
    radar_color_valid: Vec<bool>,
    tube_facts: Vec<TubeFact>,
    native_tube_indices: Vec<NativeTubeCellIndex>,
    tube_native_ids: Vec<Option<i32>>,
    tile_animations: Vec<TerrainTileAnimation>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SavedTerrainError {
    #[error("saved terrain dimensions {width}x{height} exceed the native cell table")]
    Dimensions { width: u16, height: u16 },
    #[error("saved terrain {plane} storage has {found} entries, expected {expected}")]
    Storage {
        plane: &'static str,
        expected: usize,
        found: usize,
    },
    #[error("saved terrain cell {index} has coordinate {found:?}, expected {expected:?}")]
    Coordinate {
        index: usize,
        expected: (u16, u16),
        found: (u16, u16),
    },
    #[error("saved terrain cell {index} references missing Tube {tube}")]
    TubeReference { index: usize, tube: u16 },
    #[error("saved terrain animation {index} has no allocated cell at {cell:?}")]
    AnimationCell { index: usize, cell: (u16, u16) },
    #[error("saved terrain resident assets: {0}")]
    Assets(String),
}

impl SavedTerrainTemplate {
    pub(crate) fn dimensions(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    pub(crate) fn validate(&self) -> Result<(), SavedTerrainError> {
        // Resize565C10 owns the fixed512x512 cell table. A one-sided empty
        // rectangle is not a materialized grid produced by the map owner.
        if self.width > 512 || self.height > 512 || (self.width == 0) != (self.height == 0) {
            return Err(SavedTerrainError::Dimensions {
                width: self.width,
                height: self.height,
            });
        }
        let expected = usize::from(self.width) * usize::from(self.height);
        for (plane, found) in [
            ("cells", self.cells.len()),
            ("radar validity", self.radar_color_valid.len()),
            ("native Tube indices", self.native_tube_indices.len()),
        ]
        .into_iter()
        .chain(
            self.native_allocated
                .as_ref()
                .map(|mask| ("allocation", mask.len())),
        ) {
            if found != expected {
                return Err(SavedTerrainError::Storage {
                    plane,
                    expected,
                    found,
                });
            }
        }
        if self.tube_native_ids.len() != self.tube_facts.len() {
            return Err(SavedTerrainError::Storage {
                plane: "Tube native identities",
                expected: self.tube_facts.len(),
                found: self.tube_native_ids.len(),
            });
        }
        for (index, cell) in self.cells.iter().enumerate() {
            let expected = (
                (index % usize::from(self.width)) as u16,
                (index / usize::from(self.width)) as u16,
            );
            if (cell.rx, cell.ry) != expected {
                return Err(SavedTerrainError::Coordinate {
                    index,
                    expected,
                    found: (cell.rx, cell.ry),
                });
            }
            if let Some(tube) = cell.tube_index
                && tube.as_usize() >= self.tube_facts.len()
            {
                return Err(SavedTerrainError::TubeReference {
                    index,
                    tube: tube.0,
                });
            }
        }
        // Raw signed Tube indices deliberately admit negative and out-of-range
        // values. ReadTubesINI728515 publishes source ordinals while its registry
        // grows; the compatibility Option need not equal a later revalidation.
        for (index, anim) in self.tile_animations.iter().enumerate() {
            let slot = usize::from(anim.ry) * usize::from(self.width) + usize::from(anim.rx);
            if anim.rx >= self.width
                || anim.ry >= self.height
                || self
                    .native_allocated
                    .as_ref()
                    .is_some_and(|mask| !mask[slot])
            {
                return Err(SavedTerrainError::AnimationCell {
                    index,
                    cell: (anim.rx, anim.ry),
                });
            }
        }
        Ok(())
    }

    /// Restore values before the simulation reapplies dynamic Cell bodies.
    /// The caller supplies its detached Resize dummy. No IDs, RNG, native
    /// constructors, or resident selector decisions occur at this boundary.
    pub(crate) fn restore_values(
        self,
        shared_cell_dummy: SharedCellDummy,
    ) -> Result<ResolvedTerrainGrid, SavedTerrainError> {
        self.validate()?;
        let damaged_radar_metadata = vec![None; self.cells.len()];
        Ok(ResolvedTerrainGrid {
            width: self.width,
            height: self.height,
            cells: self.cells,
            land_speed_costs: land_speed_cost_profiles(None),
            native_tmp_draw_heights: HashMap::new(),
            shared_cell_dummy,
            mutation_epoch: 0,
            native_allocated: self.native_allocated,
            radar_color_valid: self.radar_color_valid,
            damaged_radar_metadata,
            tube_facts: self.tube_facts,
            native_tube_indices: self.native_tube_indices,
            tube_native_ids: self.tube_native_ids,
            clear_tile_id: 0,
            water_tiles: crate::map::rmg::tiles::TileIds::from_keys(&Default::default()),
            tile_registry_len: None,
            dummy_accepts_smudge: false,
            bridge_set_start: None,
            high_bridge_rim_tiles: None,
            wood_bridge_set_start: None,
            tile_animations: self.tile_animations,
            destroyable_cliff_catalog: None,
            bridge_recalc_catalog: None,
            allocated_cell_order: Default::default(),
        })
    }
}

impl ResolvedTerrainGrid {
    pub(crate) fn capture_saved_template(&self) -> Result<SavedTerrainTemplate, SavedTerrainError> {
        let saved = SavedTerrainTemplate {
            width: self.width,
            height: self.height,
            cells: self.cells.clone(),
            native_allocated: self.native_allocated.clone(),
            radar_color_valid: self.radar_color_valid.clone(),
            tube_facts: self.tube_facts.clone(),
            native_tube_indices: self.native_tube_indices.clone(),
            tube_native_ids: self.tube_native_ids.clone(),
            tile_animations: self.tile_animations.clone(),
        };
        saved.validate()?;
        Ok(saved)
    }

    /// Derived asset/catalog binding shared by fresh construction and saved
    /// loading. It never materializes cells or enters the variant selector.
    pub(super) fn bind_asset_catalogs(
        &mut self,
        theater: Option<&TheaterData>,
        assets: Option<&AssetManager>,
        rules: Option<&TerrainRules>,
        lat_enabled: bool,
        cliff_back: u8,
        selector: TileVariantSelector,
    ) {
        self.land_speed_costs = land_speed_cost_profiles(rules);
        self.clear_tile_id = theater.and_then(|td| td.rmg_tiles.clear_tile).unwrap_or(0);
        self.water_tiles = theater.map_or_else(
            || crate::map::rmg::tiles::TileIds::from_keys(&Default::default()),
            crate::map::rmg::tiles::TileIds::resolve,
        );
        self.tile_registry_len = theater.map(|td| td.lookup.len());
        self.dummy_accepts_smudge =
            theater.is_some_and(|td| current_tile_permissions(&td.lookup, 0xffff).0);
        self.high_bridge_rim_tiles =
            theater.map(super::super::bridge_rim_tiles::HighBridgeRimTiles::from_theater);
        self.bridge_set_start = theater.and_then(|td| {
            td.bridge_set
                .and_then(|set| td.lookup.bounds().get(usize::from(set)))
                .map(|bounds| bounds.start)
        });
        self.wood_bridge_set_start = theater.and_then(|td| {
            td.wood_bridge_set
                .and_then(|set| td.lookup.bounds().get(usize::from(set)))
                .map(|bounds| bounds.start)
        });
        if self.cells.is_empty() {
            self.native_tmp_draw_heights.clear();
            self.destroyable_cliff_catalog = None;
            self.bridge_recalc_catalog = None;
            return;
        }
        self.native_tmp_draw_heights = load_native_tmp_draw_heights(theater, assets);
        self.destroyable_cliff_catalog = build_destroyable_cliff_catalog(theater, assets, rules);
        self.bridge_recalc_catalog = match (theater, assets, rules) {
            (Some(theater), Some(assets), Some(rules)) => {
                Some(Arc::new(BridgeRecalcCatalog::for_runtime_bridges(
                    theater,
                    assets,
                    rules,
                    lat_enabled,
                    cliff_back,
                    selector,
                )))
            }
            _ => None,
        };
    }

    /// Rebind a successfully adopted candidate to the retained process table.
    /// Asset bodies remain shared; only the selector handle changes identity.
    pub(crate) fn bind_tile_variant_selector(&mut self, selector: TileVariantSelector) {
        if let Some(catalog) = self.bridge_recalc_catalog.as_mut() {
            Arc::make_mut(catalog).bind_selector(selector);
        }
    }

    /// Bind the future-restore baseline after the actual loaded cells have
    /// visited the selector. A baseline is not itself a native loaded-cell pass
    /// and must never initialize a cold table or consume additional Main draws.
    pub(crate) fn bind_saved_template_assets(
        &mut self,
        theater: &TheaterData,
        assets: &AssetManager,
        rules: &TerrainRules,
        lat_enabled: bool,
        cliff_back: u8,
        selector: TileVariantSelector,
    ) -> Result<(), SavedTerrainError> {
        let catalog =
            self.saved_presentation_catalog(theater, assets, rules, lat_enabled, cliff_back);
        self.bind_asset_catalogs(
            Some(theater),
            Some(assets),
            Some(rules),
            lat_enabled,
            cliff_back,
            selector,
        );
        self.bind_saved_damaged_radar(&catalog)
            .map_err(|error| SavedTerrainError::Assets(error.to_string()))
    }

    /// Mouse Load5BE653..5BE65C restores actual Cell bodies before546DA0's
    /// native iterator/variant scan. Run this after dynamic cells, before final
    /// publication. Later presentation reads use the same retained table; only
    /// presentation projections change, never Cell gameplay/Tube/Anim values.
    #[expect(
        clippy::too_many_arguments,
        reason = "the existing map binder inputs plus the saved Size and selector continuation"
    )]
    pub(crate) fn finish_loaded_assets(
        &mut self,
        theater: &TheaterData,
        assets: &AssetManager,
        rules: &TerrainRules,
        lat_enabled: bool,
        cliff_back: u8,
        size_width: i32,
        selector: &mut TileVariantSelectionContext<'_, '_>,
    ) -> Result<(), SavedTerrainError> {
        let clear_tile = theater.rmg_tiles.clear_tile.unwrap_or(0);
        let mut catalog =
            self.saved_presentation_catalog(theater, assets, rules, lat_enabled, cliff_back);
        catalog.bind_selector(selector.selector());
        let mut visited = Vec::new();
        for cell in self.native_cell_iterator(size_width) {
            catalog
                .select_loaded_variant(cell, clear_tile, selector)
                .map_err(|error| SavedTerrainError::Assets(error.to_string()))?;
            visited.push(usize::from(cell.ry) * usize::from(self.width) + usize::from(cell.rx));
        }
        self.bind_asset_catalogs(
            Some(theater),
            Some(assets),
            Some(rules),
            lat_enabled,
            cliff_back,
            selector.selector(),
        );
        // GetRadarColor47C24A/tactical547F9D are downstream presentation
        // consumers, not writes performed by546DA0 itself. The scan already
        // admitted every selector entry, including cold sentinel/count1.
        for index in visited {
            self.refresh_cell_presentation(
                &catalog,
                index,
                Some(&mut || {
                    panic!("loaded-cell scan must initialize every admitted selector consumer")
                }),
            )
            .map_err(|error| SavedTerrainError::Assets(error.to_string()))?;
        }
        Ok(())
    }

    fn saved_presentation_catalog(
        &self,
        theater: &TheaterData,
        assets: &AssetManager,
        rules: &TerrainRules,
        lat_enabled: bool,
        cliff_back: u8,
    ) -> BridgeRecalcCatalog {
        let clear_tile = i32::from(theater.rmg_tiles.clear_tile.unwrap_or(0));
        // Reuse the resident reader even for theaters without a bridge family.
        // Its separate pristine admission records do not run Recalc callbacks;
        // presentation files may include animated and Tunnel tiles safely.
        let ids = self.iter().map(|cell| {
            if cell.final_tile_index < 0
                || cell.final_tile_index == 0xffff
                || cell.final_tile_index as usize >= theater.lookup.len()
            {
                clear_tile
            } else {
                cell.final_tile_index
            }
        });
        BridgeRecalcCatalog::for_tiles(theater, assets, rules, lat_enabled, cliff_back, ids)
    }
}
