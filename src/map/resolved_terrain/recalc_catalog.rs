//! Resident inputs for live bridge terrain reconstruction.
//!
//! Original568E40/569760 constructors and586990 also recalculate ordinary
//! terrain below/alongside bridges. Retain registered pristine heads plus
//! independent presentation files; missing sources never become sparse entries.

use super::build::{
    TileMetadata, merge_tmp_file_metadata, metadata_from_set_name,
    ordinary_variant_selection_enabled,
};
use super::tubes::auto_tube_direction_from_bases;
use super::*;
use crate::map::tile_variant_selector::TileVariantSelector;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum BridgeRecalcCatalogError {
    #[error("bridge Recalc has no resident metadata for registered tile {tile}")]
    Uncached { tile: i32 },
    #[error("bridge Recalc tile {tile} is unavailable: {reason}")]
    Unavailable { tile: u16, reason: String },
    #[error("bridge Recalc tile {tile} requires unsupported {effect}")]
    Unsupported { tile: u16, effect: &'static str },
    #[error(
        "bridge tile replacement requires a valid resident tile/subtile, received {tile}/{sub}"
    )]
    InvalidReplacement { tile: i32, sub: u8 },
}

type ResidentTmp = Result<Arc<ResidentBridgeTmp>, BridgeRecalcCatalogError>;

/// Store real entries once, with an explicit out-of-bounds metadata result.
/// Allocating 256 copies for every registered type is unnecessary:47D2B0
/// checks the unsigned subtile against the pristine template's actual size.
#[derive(Debug)]
struct ResidentBridgeTmp {
    entries: Box<[TileMetadata]>,
    invalid_subtile: TileMetadata,
    raw_pointer_words: Box<[u32]>,
}

impl ResidentBridgeTmp {
    fn metadata(&self, sub: u8) -> &TileMetadata {
        self.entries
            .get(usize::from(sub))
            .unwrap_or(&self.invalid_subtile)
    }

    fn radar(&self, tile: u16, sub: u8) -> Result<RadarColorMetadata, BridgeRecalcCatalogError> {
        if let Some(entry) = self.entries.get(usize::from(sub)) {
            return Ok(RadarColorMetadata {
                left: entry.radar_left,
                right: entry.radar_right,
                valid: entry.subtile_entry_valid == Some(true),
            });
        }
        match self.raw_pointer_words.get(usize::from(sub)) {
            Some(0) => Ok(RadarColorMetadata {
                left: [0; 3],
                right: [0; 3],
                valid: false,
            }),
            _ => Err(BridgeRecalcCatalogError::Unsupported {
                tile,
                effect: "non-null or unavailable radar pointer beyond the selected TMP entry table",
            }),
        }
    }

    fn tactical(&self, sub: u8) -> &TileMetadata {
        // Original547F9D: wrap by the selected file's template size, not the
        // pristine file's size or the strict Recalc entry-validation result.
        &self.entries[usize::from(sub) % self.entries.len()]
    }
}

fn read_resident_tmp(
    assets: &crate::assets::asset_manager::AssetManager,
    rules: &TerrainRules,
    tile: u16,
    filename: &str,
    set_name: Option<&str>,
    set: Option<u16>,
    warned: &mut HashSet<u8>,
) -> Result<ResidentBridgeTmp, BridgeRecalcCatalogError> {
    let bytes = assets
        .get(filename)
        .ok_or_else(|| BridgeRecalcCatalogError::Unavailable {
            tile,
            reason: format!("missing {filename}"),
        })?;
    let tmp =
        TmpFile::from_bytes(&bytes).map_err(|error| BridgeRecalcCatalogError::Unavailable {
            tile,
            reason: format!("{filename}: {error}"),
        })?;
    let count = tmp.tiles.len().min(256);
    if count == 0 {
        return Err(BridgeRecalcCatalogError::Unavailable {
            tile,
            reason: format!("{filename}: empty TMP template"),
        });
    }
    let mut metadata = |sub| {
        let mut entry = metadata_from_set_name(set_name, set);
        entry.tmp_file_valid = true;
        merge_tmp_file_metadata(&mut entry, &tmp, sub, Some(rules), warned);
        entry
    };
    let entries = (0..count).map(|sub| metadata(sub as u8)).collect();
    let invalid_subtile = metadata(count.min(255) as u8);
    // Radar47C2CF reads this table directly, unlike tactical547F33's modulo.
    // Retain the words following the actual table as well, so a missing
    // selected-file entry is not automatically misclassified as a null one.
    let raw_pointer_words = bytes[16..]
        .as_chunks::<4>()
        .0
        .iter()
        .take(256)
        .map(|word| u32::from_le_bytes(*word))
        .collect();
    Ok(ResidentBridgeTmp {
        entries,
        invalid_subtile,
        raw_pointer_words,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct BridgeRecalcCatalog {
    heads: HashMap<u16, ResidentTmp>,
    files: HashMap<u16, Box<[ResidentTmp]>>,
    selector: TileVariantSelector,
    permissions: Vec<(bool, bool)>,
    registered_names: Vec<Vec<u8>>,
    pub(super) terrain_rules: TerrainRules,
    pub(super) lat_config: Option<lat::LatConfig>,
    pub(super) slope_config: Option<lat::SlopeFixupConfig>,
    pub(super) cliff_back_impassability: u8,
    automatic_tube_bases: [i32; 4],
    wood_bridge_start: Option<u16>,
}

impl BridgeRecalcCatalog {
    fn file(&self, tile: u16, variant: u8) -> Result<&ResidentBridgeTmp, BridgeRecalcCatalogError> {
        self.files
            .get(&tile)
            .ok_or(BridgeRecalcCatalogError::Uncached {
                tile: i32::from(tile),
            })?
            .get(usize::from(variant))
            .ok_or(BridgeRecalcCatalogError::Unavailable {
                tile,
                reason: format!("missing registered TMP file {variant}"),
            })?
            .as_deref()
            .map_err(Clone::clone)
    }

    pub(super) fn tactical_metadata(
        &self,
        tile: u16,
        sub: u8,
        variant: u8,
    ) -> Result<TileMetadata, BridgeRecalcCatalogError> {
        Ok(self.file(tile, variant)?.tactical(sub).clone())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "distinct native retained/draw subtile and Main continuation inputs"
    )]
    fn presentation(
        &self,
        tile: u16,
        retained_sub: u8,
        draw_sub: u8,
        rx: u16,
        ry: u16,
        clear_fallback: bool,
        raw_main_draw: &mut dyn FnMut() -> u32,
    ) -> Result<BridgePresentation, BridgeRecalcCatalogError> {
        let pristine = self.file(tile, 0)?;
        let count = self.files[&tile].len() as u8;
        let source = pristine.tactical(retained_sub);
        let damaged = !clear_fallback && source.has_damaged_data;
        // Native sentinel branches bypass the damaged gate altogether.
        let variant = if clear_fallback || ordinary_variant_selection_enabled(count, false, damaged)
        {
            self.selector.select_variant(
                (i32::from(rx), i32::from(ry)),
                retained_sub,
                (source.template_width_cells, source.template_height_cells),
                count,
                raw_main_draw,
            )
        } else {
            0
        };
        self.presentation_for_variant(tile, retained_sub, draw_sub, clear_fallback, variant)
    }

    fn presentation_for_variant(
        &self,
        tile: u16,
        retained_sub: u8,
        draw_sub: u8,
        clear_fallback: bool,
        variant: u8,
    ) -> Result<BridgePresentation, BridgeRecalcCatalogError> {
        let pristine = self.file(tile, 0)?;
        let damaged = !clear_fallback && pristine.tactical(retained_sub).has_damaged_data;
        let count = self.files[&tile].len();
        let selected = self.file(tile, variant)?;
        let tactical = self.tactical_metadata(tile, draw_sub, variant)?;
        Ok(BridgePresentation {
            radar: selected.radar(tile, retained_sub)?,
            damaged_radar: if damaged && count > 1 {
                Some(
                    self.file(tile, 1)
                        .and_then(|file| file.radar(tile, retained_sub)),
                )
            } else {
                None
            },
            offset: [tactical.render_offset_x, tactical.render_offset_y],
            has_damaged_data: damaged,
            variant,
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn with_fixture_files(
        mut self,
        tile: u16,
        assets: &crate::assets::asset_manager::AssetManager,
        names: &[&str],
        table: [u8; 64],
    ) -> Self {
        let mut warned = HashSet::new();
        self.files.insert(
            tile,
            names
                .iter()
                .map(|name| {
                    read_resident_tmp(
                        assets,
                        &self.terrain_rules,
                        tile,
                        name,
                        None,
                        None,
                        &mut warned,
                    )
                    .map(Arc::new)
                })
                .collect(),
        );
        self.selector = TileVariantSelector::from_initialized(table);
        self
    }

    pub(super) fn for_runtime_bridges(
        theater: &TheaterData,
        assets: &crate::assets::asset_manager::AssetManager,
        rules: &TerrainRules,
        lat_enabled: bool,
        cliff_back_impassability: u8,
        selector: TileVariantSelector,
    ) -> Self {
        // Span construction and the queued Recalc rectangle have no family
        // gate. Registered ordinary sources and later LAT replacements must
        // remain available after TheaterData/AssetManager leave scope.
        let has_bridge_family = theater.bridge_set.is_some() || theater.wood_bridge_set.is_some();
        let mut catalog = Self::for_tiles(
            theater,
            assets,
            rules,
            lat_enabled,
            cliff_back_impassability,
            (0..theater.lookup.len() as i32).filter(|_| has_bridge_family),
        );
        catalog.selector = selector;
        catalog
    }

    pub(super) fn for_tiles(
        theater: &TheaterData,
        assets: &crate::assets::asset_manager::AssetManager,
        rules: &TerrainRules,
        lat_enabled: bool,
        cliff_back_impassability: u8,
        ids: impl IntoIterator<Item = i32>,
    ) -> Self {
        //545150 supplies FileName+ordinal before appending the extension;
        //AbstractType41088D keeps the first24 bytes.544CE0 searches declaration
        //order with case-insensitive C-string comparison (ToTile465CC0).
        let registered_names = (0..theater.lookup.len())
            .map(|id| {
                let name = theater.lookup.filename(id as i32).unwrap_or("");
                let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
                stem.as_bytes().iter().copied().take(24).collect()
            })
            .collect();
        let permissions = (0..theater.lookup.len())
            .map(|id| {
                (
                    theater.lookup.is_morphable(id as u16),
                    theater.lookup.allows_tiberium(id as u16),
                )
            })
            .collect();
        let ini = crate::rules::ini_parser::IniFile::from_bytes(&theater.ini_data);
        let mut heads = HashMap::new();
        let mut files = HashMap::new();
        let mut warned = HashSet::new();
        for id in ids {
            let Ok(tile) = u16::try_from(id) else {
                continue;
            };
            if usize::from(tile) >= theater.lookup.len() || heads.contains_key(&tile) {
                continue;
            }
            let set = theater.lookup.tileset_index(tile);
            let set_name = set.and_then(|set| theater.lookup.set_name(set));
            let pristine = theater
                .lookup
                .filename(id)
                .ok_or_else(|| BridgeRecalcCatalogError::Unavailable {
                    tile,
                    reason: "no pristine filename".into(),
                })
                .and_then(|filename| {
                    read_resident_tmp(assets, rules, tile, filename, set_name, set, &mut warned)
                })
                .map(Arc::new);
            let result = (|| {
                let ini = ini
                    .as_ref()
                    .map_err(|error| BridgeRecalcCatalogError::Unavailable {
                        tile,
                        reason: format!("theater INI: {error}"),
                    })?;
                let set = theater.lookup.tileset_index(tile);
                let set_name = set.and_then(|set| theater.lookup.set_name(set));
                let set_section = set.map(|set| format!("TileSet{set:04}"));
                // This narrow adapter supports the independently proved
                // absence of shadow policy. It does not interpret TS shadow
                // options or silently suppress a declared native callback.
                if [set_section.as_deref(), set_name]
                    .into_iter()
                    .flatten()
                    .filter_map(|name| ini.section(name))
                    .any(|section| {
                        section.is_present("ShadowCaster") || section.is_present("ShadowTiles")
                    })
                {
                    return Err(BridgeRecalcCatalogError::Unsupported {
                        tile,
                        effect: "declared shadow policy",
                    });
                }
                if theater.lookup.tile_anim(tile).is_some() {
                    return Err(BridgeRecalcCatalogError::Unsupported {
                        tile,
                        effect: "terrain animation attachment",
                    });
                }
                let pristine = pristine.as_ref().map_err(Clone::clone)?;
                if pristine
                    .entries
                    .iter()
                    .any(|entry| entry.land_type == LandType::Tunnel.as_index())
                {
                    return Err(BridgeRecalcCatalogError::Unsupported {
                        tile,
                        effect: "Tube land",
                    });
                }
                Ok(Arc::clone(pristine))
            })();
            let mut siblings = vec![pristine];
            for variant in 1..theater.lookup.total_file_count(tile) {
                let filename = theater
                    .lookup
                    .filename_for_variant(tile, variant)
                    .expect("registered variant has a filename");
                let set = theater.lookup.tileset_index(tile);
                siblings.push(
                    read_resident_tmp(
                        assets,
                        rules,
                        tile,
                        filename,
                        set.and_then(|set| theater.lookup.set_name(set)),
                        set,
                        &mut warned,
                    )
                    .map(Arc::new),
                );
            }
            files.insert(tile, siblings.into_boxed_slice());
            heads.insert(tile, result);
        }
        let ids = crate::map::rmg::tiles::TileIds::from_keys(&theater.rmg_tiles);
        Self {
            heads,
            files,
            selector: TileVariantSelector::default(),
            permissions,
            registered_names,
            terrain_rules: rules.clone(),
            lat_config: lat_enabled.then(|| lat::LatConfig::from_tile_ids(&ids)),
            slope_config: lat_enabled.then(|| lat::SlopeFixupConfig::from_tile_ids(&ids)),
            cliff_back_impassability,
            automatic_tube_bases: theater.automatic_tube_bases,
            wood_bridge_start: theater
                .wood_bridge_set
                .and_then(|set| theater.lookup.bounds().get(usize::from(set)))
                .map(|bounds| bounds.start),
        }
    }

    fn presentation_tile(&self, cell: &ResolvedTerrainCell, clear_tile: u16) -> (u16, bool) {
        let fallback = cell.final_tile_index < 0
            || cell.final_tile_index == 0xffff
            || cell.final_tile_index as usize >= self.permissions.len();
        (
            if fallback {
                clear_tile
            } else {
                cell.final_tile_index as u16
            },
            fallback,
        )
    }

    pub(super) fn bind_selector(&mut self, selector: TileVariantSelector) {
        self.selector = selector;
    }

    /// Mouse Load5BE657 calls546DA0 after Cell bodies are restored. The pass
    /// only selects resident files; it never calls Fill or RecalcAttributes.
    /// Native executed controls: bridge_recalc_presentation.json/load_selector_cases.
    pub(super) fn select_loaded_variant(
        &self,
        cell: &ResolvedTerrainCell,
        clear_tile: u16,
        selector: &mut TileVariantSelectionContext<'_, '_>,
    ) -> Result<u8, BridgeRecalcCatalogError> {
        let fallback = cell.final_tile_index == 0xffff
            || cell.final_tile_index >= self.permissions.len() as i32;
        let tile = if fallback {
            clear_tile
        } else {
            u16::try_from(cell.final_tile_index).map_err(|_| {
                BridgeRecalcCatalogError::Uncached {
                    tile: cell.final_tile_index,
                }
            })?
        };
        let pristine = self.file(tile, 0)?;
        let count = self.files[&tile].len() as u8;
        if !fallback && count <= 1 {
            return Ok(0);
        }
        let source = pristine.tactical(cell.final_sub_tile);
        if !fallback && source.has_damaged_data {
            return Ok(u8::from(cell.bridge_facts.raw_flags & 0x2000 != 0));
        }
        Ok(selector.select_variant(
            i32::from(cell.rx),
            i32::from(cell.ry),
            cell.final_sub_tile,
            source.template_width_cells,
            source.template_height_cells,
            count,
        ))
    }

    pub(super) fn metadata(
        &self,
        tile: i32,
        sub: u8,
    ) -> Result<Option<TileMetadata>, BridgeRecalcCatalogError> {
        if tile < 0 || tile == 0xFFFF || tile as usize >= self.permissions.len() {
            return Ok(None);
        }
        let entries = self
            .heads
            .get(&(tile as u16))
            .ok_or(BridgeRecalcCatalogError::Uncached { tile })?
            .as_ref()
            .map_err(Clone::clone)?;
        Ok(Some(entries.metadata(sub).clone()))
    }

    pub(super) fn current_permissions(&self, tile: i32) -> (bool, bool) {
        if tile < 0 || tile as usize >= self.permissions.len() {
            (self.permissions.first().is_some_and(|row| row.0), true)
        } else {
            self.permissions[tile as usize]
        }
    }

    pub(super) fn automatic_tube_direction(&self, tile: i32) -> Option<u8> {
        auto_tube_direction_from_bases(tile, self.automatic_tube_bases)
    }

    pub(super) fn is_wood_bridge_repair_tile(&self, tile: i32) -> bool {
        tile >= 0
            && tile != 0xFFFF
            && self
                .wood_bridge_start
                .is_some_and(|start| tile >= i32::from(start) && tile < i32::from(start) + 16)
    }
}

struct BridgePresentation {
    radar: RadarColorMetadata,
    damaged_radar: Option<Result<RadarColorMetadata, BridgeRecalcCatalogError>>,
    offset: [i32; 2],
    has_damaged_data: bool,
    variant: u8,
}

impl ResolvedTerrainGrid {
    /// Unit73F12E..73F1D9 reads the CURRENT registered type's pristine
    /// dimensions, even when that type is not admitted by the runtime Recalc
    /// effect adapter. Registration546B46/546B50 stores the header low bytes.
    pub(crate) fn current_tile_dimensions(&self, tile: i32) -> Result<(u8, u8), String> {
        let catalog = self
            .bridge_recalc_catalog
            .as_ref()
            .ok_or("tile dimensions query has no resident registry")?;
        let tile = u16::try_from(tile).map_err(|_| "invalid current tile dimensions index")?;
        let pristine = catalog
            .file(tile, 0)
            .map_err(|e| format!("tile dimensions: {e:?}"))?;
        let metadata = &pristine.invalid_subtile;
        Ok((
            metadata.template_width_cells as u8,
            metadata.template_height_cells as u8,
        ))
    }

    /// Original544CE0/465CC0. A missing name is a null ToTile pointer, not an
    /// invalid tile index; the input itself is never truncated to24 bytes.
    pub(crate) fn resolve_registered_tile_name(
        &self,
        name: &str,
    ) -> Result<Option<u16>, &'static str> {
        let catalog = self
            .bridge_recalc_catalog
            .as_ref()
            .ok_or("tile name query has no resident registry")?;
        Ok(catalog
            .registered_names
            .iter()
            .position(|registered| registered.eq_ignore_ascii_case(name.as_bytes()))
            .map(|index| index as u16))
    }

    ///47C76B tests Morphable only for a valid CURRENT tile index. Unlike the
    ///smudge receiver, invalid indices bypass this gate rather than using tile0.
    pub(crate) fn tile_allows_morph_placement(&self, tile: i32) -> Result<bool, &'static str> {
        let catalog = self
            .bridge_recalc_catalog
            .as_ref()
            .ok_or("Morphable query has no resident registry")?;
        Ok(usize::try_from(tile)
            .ok()
            .and_then(|i| catalog.permissions.get(i))
            .is_none_or(|row| row.0))
    }

    /// The native56EB80 raw +38 store, admitted before changing live state.
    /// This live bridge adapter accepts the stock valid middle entries. The
    /// shared scalar Recalc still models native invalid/sparse branches.
    pub(crate) fn write_resident_bridge_tile(
        &mut self,
        index: usize,
        tile: i32,
        overlay: FinalizedOverlayCell,
        overlay_types: &OverlayTypeRegistry,
    ) -> Result<(), LoadCellRecalcError<&'static str>> {
        let catalog = self
            .bridge_recalc_catalog
            .as_ref()
            .ok_or(LoadCellRecalcError::MissingResidentInputs)?;
        let cell = self
            .cells
            .get(index)
            .ok_or(LoadCellRecalcError::CellIndexOutOfBounds { index })?;
        let sub = cell.final_sub_tile;
        let metadata = catalog
            .metadata(tile, sub)
            .map_err(LoadCellRecalcError::ResidentInput)?;
        if !metadata.is_some_and(|metadata| metadata.subtile_entry_valid == Some(true)) {
            return Err(LoadCellRecalcError::ResidentInput(
                BridgeRecalcCatalogError::InvalidReplacement { tile, sub },
            ));
        }
        match overlay.identity() {
            NO_OVERLAY_IDENTITY => {}
            identity @ 0..=254 => {
                let overlay_id = identity as u8;
                if overlay_types.flags(overlay_id).is_none() {
                    return Err(LoadCellRecalcError::MissingOverlayType { overlay_id });
                }
            }
            identity => return Err(LoadCellRecalcError::MalformedOverlayIdentity { identity }),
        }
        self.cells[index].final_tile_index = tile;
        Ok(())
    }

    /// Refresh final-type queries after47D2B0, without replacing its retained
    /// pristine land/slope/dimensions. Native radar47C060 reads current+38/+11A.
    /// Radar reads the selected file's raw table; tactical draw wraps its
    /// subtile by the selected dimensions. Neither changes pristine gameplay
    /// attributes. A sentinel uses ClearTile while radar retains raw Cell+11A.
    pub(crate) fn refresh_resident_bridge_presentation(
        &mut self,
        index: usize,
        raw_main_draw: &mut dyn FnMut() -> u32,
    ) -> Result<(), LoadCellRecalcError<&'static str>> {
        let catalog = self
            .bridge_recalc_catalog
            .clone()
            .ok_or(LoadCellRecalcError::MissingResidentInputs)?;
        self.refresh_cell_presentation(&catalog, index, Some(raw_main_draw))
    }

    /// Snapshot scalar restoration retains its saved selection until the
    /// ordered loaded-cell pass has advanced the candidate process selector.
    pub(super) fn refresh_saved_cell_presentation(
        &mut self,
        index: usize,
    ) -> Result<(), LoadCellRecalcError<&'static str>> {
        let catalog = self
            .bridge_recalc_catalog
            .clone()
            .ok_or(LoadCellRecalcError::MissingResidentInputs)?;
        self.refresh_cell_presentation(&catalog, index, None)
    }

    pub(super) fn refresh_cell_presentation(
        &mut self,
        catalog: &BridgeRecalcCatalog,
        index: usize,
        raw_main_draw: Option<&mut dyn FnMut() -> u32>,
    ) -> Result<(), LoadCellRecalcError<&'static str>> {
        let cell = self
            .cells
            .get(index)
            .ok_or(LoadCellRecalcError::CellIndexOutOfBounds { index })?;
        let (tile, clear_fallback) = catalog.presentation_tile(cell, self.clear_tile_id);
        let draw_sub = if clear_fallback {
            0
        } else {
            cell.final_sub_tile
        };
        let metadata = match raw_main_draw {
            Some(draw) => catalog.presentation(
                tile,
                cell.final_sub_tile,
                draw_sub,
                cell.rx,
                cell.ry,
                clear_fallback,
                draw,
            ),
            None => catalog.presentation_for_variant(
                tile,
                cell.final_sub_tile,
                draw_sub,
                clear_fallback,
                cell.variant,
            ),
        }
        .map_err(LoadCellRecalcError::ResidentInput)?;
        let cell = &mut self.cells[index];
        cell.radar_left = metadata.radar.left;
        cell.radar_right = metadata.radar.right;
        cell.render_offset_x = metadata.offset[0];
        cell.render_offset_y = metadata.offset[1];
        cell.has_damaged_data = metadata.has_damaged_data;
        cell.variant = metadata.variant;
        cell.filled_clear = clear_fallback;
        self.radar_color_valid[index] = metadata.radar.valid;
        self.damaged_radar_metadata[index] = metadata.damaged_radar;
        Ok(())
    }

    /// Reconstruct only the asset-derived damaged sibling plane. The saved
    /// baseline's complete Cell values and explicit radar validity stay intact.
    pub(super) fn bind_saved_damaged_radar(
        &mut self,
        catalog: &BridgeRecalcCatalog,
    ) -> Result<(), BridgeRecalcCatalogError> {
        for (index, cell) in self.cells.iter().enumerate() {
            if self
                .native_allocated
                .as_ref()
                .is_some_and(|mask| !mask[index])
            {
                continue;
            }
            let (tile, fallback) = catalog.presentation_tile(cell, self.clear_tile_id);
            let metadata = catalog.presentation_for_variant(
                tile,
                cell.final_sub_tile,
                if fallback { 0 } else { cell.final_sub_tile },
                fallback,
                cell.variant,
            )?;
            self.damaged_radar_metadata[index] = metadata.damaged_radar;
        }
        Ok(())
    }

    /// Resident scalar47D2B0 adapter for the admitted stock bridge source set.
    /// Caller retains ownership of ordered overlay/radar/navigation publication.
    /// The grid is never moved out of its Simulation owner for a callback.
    pub(crate) fn recalc_resident_bridge_cell(
        &mut self,
        index: usize,
        overlay: FinalizedOverlayCell,
        level_override: i32,
        overlay_types: &OverlayTypeRegistry,
        current_bounds: Option<PlayfieldBounds>,
    ) -> Result<LoadCellRecalcOutcome, LoadCellRecalcError<&'static str>> {
        let catalog = self
            .bridge_recalc_catalog
            .clone()
            .ok_or(LoadCellRecalcError::MissingResidentInputs)?;
        let cell = self
            .cells
            .get(index)
            .ok_or(LoadCellRecalcError::CellIndexOutOfBounds { index })?;
        // The early-overlay branch writes land before querying pristine TMP
        // slope. Reject missing/unadmitted resident inputs before that write;
        // native invalid/sparse inputs remain admitted fallback cases.
        catalog
            .metadata(cell.final_tile_index, cell.final_sub_tile)
            .map_err(LoadCellRecalcError::ResidentInput)?;
        let mut state =
            LoadCellRecalcState::for_resident_bridge(&catalog, overlay_types, current_bounds);
        self.recalc_cell_attributes(
            &mut state,
            index,
            overlay,
            level_override,
            &mut RejectUnadmittedEffects,
        )
    }
}

/// The admitted stock source set has no lifecycle callbacks. Preserve an error
/// boundary if a future policy change invalidates that proof; never no-op one.
struct RejectUnadmittedEffects;

impl LoadCellRecalcEffects for RejectUnadmittedEffects {
    type Error = &'static str;

    fn allocate_automatic_tube(
        &mut self,
        _: AutomaticTubeRequest,
    ) -> Result<AutomaticTubeAllocation, Self::Error> {
        Err("runtime bridge Recalc requires an unadmitted Tube constructor")
    }

    fn construct_terrain_attached_anim(
        &mut self,
        _: &TerrainTileAnimation,
    ) -> Result<(), Self::Error> {
        Err("runtime bridge Recalc requires an unadmitted terrain animation")
    }
}
