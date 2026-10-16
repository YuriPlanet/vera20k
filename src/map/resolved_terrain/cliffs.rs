//! Destroyable cliff collapse: the active theater's sparse replacement
//! templates and the runtime tile replacement they drive.

use super::build::{TileMetadata, canonical_ramp_from_slope_type, load_tile_metadata};
use super::*;

#[derive(Debug, Clone)]
struct DynamicTilePrototype {
    metadata: TileMetadata,
    accepts_smudge: bool,
    allows_tiberium: bool,
}

#[derive(Debug, Clone)]
struct SparseTileTemplate {
    tile_id: u16,
    width: u8,
    height: u8,
    entries: Vec<Option<DynamicTilePrototype>>,
}

#[derive(Debug, Clone)]
pub(super) struct DestroyableCliffCatalog {
    destroyable_start: u16,
    old: [SparseTileTemplate; 2],
    replacements: [SparseTileTemplate; 4],
    lat_config: lat::LatConfig,
    slope_config: lat::SlopeFixupConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DestroyableCliffFamily {
    A,
    B,
}

/// The destroyable-cliff IsoTile a cell belongs to
/// ([`ResolvedTerrainGrid::destroyable_cliff_tile`]).
pub(crate) struct DestroyableCliffTile<'a> {
    /// The tile's top-left cell: the cell less its subtile's column and row.
    pub origin: (i16, i16),
    pub width: u8,
    pub height: u8,
    entries: &'a [Option<DynamicTilePrototype>],
}

impl DestroyableCliffTile<'_> {
    /// Whether the tile's TMP has an image for this subtile (a nonzero
    /// offset in its table).
    pub(crate) fn has_image(&self, column: u8, row: u8) -> bool {
        self.entries
            .get(usize::from(row) * usize::from(self.width) + usize::from(column))
            .is_some_and(Option::is_some)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DestroyableCliffMutation {
    #[cfg(any(test, feature = "test-support"))]
    pub family: DestroyableCliffFamily,
    #[cfg(any(test, feature = "test-support"))]
    pub origin: (i16, i16),
    pub original_footprint: Vec<(u16, u16)>,
    pub animation_cells: Vec<(i16, i16)>,
    pub changed_cells: Vec<(u16, u16)>,
}

impl ResolvedTerrainGrid {
    pub(crate) fn is_destroyable_cliff(&self, rx: u16, ry: u16) -> bool {
        let Some(catalog) = self.destroyable_cliff_catalog.as_ref() else {
            return false;
        };
        self.cell(rx, ry).is_some_and(|cell| {
            cell.final_tile_index == i32::from(catalog.destroyable_start)
                || cell.final_tile_index == i32::from(catalog.destroyable_start) + 1
        })
    }

    /// The destroyable-cliff tile under `(rx, ry)`, as `MapClass @
    /// 0x00586FC0` reads it: the cell's IsoTile (`+0x38`), whose subtile
    /// (`+0x11A`) splits into column and row by the tile's width
    /// (`0x0058705A..0x00587095`). None off a destroyable cliff.
    pub(crate) fn destroyable_cliff_tile(
        &self,
        rx: u16,
        ry: u16,
    ) -> Option<DestroyableCliffTile<'_>> {
        let catalog = self.destroyable_cliff_catalog.as_ref()?;
        let cell = self.cell(rx, ry)?;
        let template = match cell.final_tile_index {
            tile if tile == i32::from(catalog.destroyable_start) => &catalog.old[0],
            tile if tile == i32::from(catalog.destroyable_start) + 1 => &catalog.old[1],
            _ => return None,
        };
        if template.width == 0 {
            return None;
        }
        let column = cell.final_sub_tile % template.width;
        let row = cell.final_sub_tile / template.width;
        Some(DestroyableCliffTile {
            origin: (
                (rx as i16).wrapping_sub(i16::from(column)),
                (ry as i16).wrapping_sub(i16::from(row)),
            ),
            width: template.width,
            height: template.height,
            entries: &template.entries,
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn test_install_destroyable_cliff_catalog(&mut self, destroyable_start: u16) {
        fn prototype(template_height: u8, slope_type: u8) -> DynamicTilePrototype {
            DynamicTilePrototype {
                metadata: TileMetadata {
                    tileset_index: Some(1),
                    land_type: LandType::Clear.as_index(),
                    yr_cell_land_type: LandType::Clear.as_index(),
                    slope_type,
                    template_height,
                    terrain_class: TerrainClass::Clear,
                    radar_left: [slope_type, 1, 2],
                    radar_right: [slope_type, 3, 4],
                    ..TileMetadata::default()
                },
                accepts_smudge: true,
                allows_tiberium: false,
            }
        }
        fn template(
            tile_id: u16,
            width: u8,
            height: u8,
            template_height: u8,
            slope_type: u8,
            holes: &[usize],
        ) -> SparseTileTemplate {
            let mut entries = vec![
                Some(prototype(template_height, slope_type));
                usize::from(width) * usize::from(height)
            ];
            for &index in holes {
                entries[index] = None;
            }
            SparseTileTemplate {
                tile_id,
                width,
                height,
                entries,
            }
        }
        self.destroyable_cliff_catalog = Some(DestroyableCliffCatalog {
            destroyable_start,
            old: [
                template(destroyable_start, 6, 4, 1, 0, &[0, 5, 18, 23]),
                template(destroyable_start + 1, 4, 6, 1, 0, &[0, 3, 20, 23]),
            ],
            replacements: [
                template(destroyable_start + 100, 3, 4, 1, 1, &[0, 9]),
                template(destroyable_start + 101, 3, 4, 1, 2, &[2, 11]),
                template(destroyable_start + 102, 4, 3, 1, 3, &[8, 11]),
                template(destroyable_start + 103, 4, 3, 1, 4, &[0, 3]),
            ],
            lat_config: lat::LatConfig {
                grounds: Vec::new(),
            },
            slope_config: lat::SlopeFixupConfig {
                ramp_base: -1,
                ramp_smooth: -1,
            },
        });
    }

    /// gamemd-derived: `MapClass::CollapseDestroyableCliff @ 0x00581140`
    /// terrain half. The world owner surrounds this with zones, detach/dirty,
    /// and AnimClass construction so all non-grid authorities remain ordered.
    pub(crate) fn collapse_destroyable_cliff_terrain(
        &mut self,
        rx: u16,
        ry: u16,
        mut clear_replacement_cell: impl FnMut(u16, u16),
    ) -> Option<DestroyableCliffMutation> {
        let catalog = self.destroyable_cliff_catalog.clone()?;
        let selected = self.cell(rx, ry)?;
        let family = match selected.final_tile_index {
            tile if tile == i32::from(catalog.destroyable_start) => DestroyableCliffFamily::A,
            tile if tile == i32::from(catalog.destroyable_start) + 1 => DestroyableCliffFamily::B,
            _ => return None,
        };
        let old = &catalog.old[match family {
            DestroyableCliffFamily::A => 0,
            DestroyableCliffFamily::B => 1,
        }];
        let sub_tile = selected.final_sub_tile;
        let (sub_x, sub_y) = match family {
            DestroyableCliffFamily::A => (sub_tile % 6, sub_tile / 6),
            DestroyableCliffFamily::B => (sub_tile & 3, sub_tile >> 2),
        };
        let origin = (
            (rx as i16).wrapping_sub(i16::from(sub_x)),
            (ry as i16).wrapping_sub(i16::from(sub_y)),
        );

        let coord_for = |x: u8, y: u8| -> Option<(u16, u16)> {
            let x = origin.0.wrapping_add(i16::from(x));
            let y = origin.1.wrapping_add(i16::from(y));
            let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
                return None;
            };
            Some((x, y))
        };
        let mut original_footprint = Vec::new();
        for y in 0..old.height {
            for x in 0..old.width {
                let index = usize::from(y) * usize::from(old.width) + usize::from(x);
                let Some(prototype) = old.entries.get(index).and_then(Option::as_ref) else {
                    continue;
                };
                let Some(coord) = coord_for(x, y) else {
                    continue;
                };
                if !self
                    .cell(coord.0, coord.1)
                    .is_some_and(|cell| !cell.outside_playfield)
                {
                    continue;
                }
                original_footprint.push(coord);
                if let Some(cell) = self.cell_mut(coord.0, coord.1)
                    && cell.final_tile_index == i32::from(old.tile_id)
                    && usize::from(cell.final_sub_tile) == index
                {
                    cell.final_tile_index = 0xFFFF;
                    cell.final_sub_tile = 0;
                    cell.level = (cell.level as i8)
                        .wrapping_sub(prototype.metadata.template_height as i8)
                        as u8;
                }
            }
        }

        let stamps: [(usize, (i16, i16)); 2] = match family {
            DestroyableCliffFamily::A => [(0, origin), (1, (origin.0.wrapping_add(3), origin.1))],
            DestroyableCliffFamily::B => [(3, origin), (2, (origin.0, origin.1.wrapping_add(3)))],
        };
        let mut changed_cells = Vec::new();
        for (template_index, anchor) in stamps {
            let template = &catalog.replacements[template_index];
            for y in 0..template.height {
                for x in 0..template.width {
                    let index = usize::from(y) * usize::from(template.width) + usize::from(x);
                    let Some(prototype) = template.entries.get(index).and_then(Option::as_ref)
                    else {
                        continue;
                    };
                    let x = anchor.0.wrapping_add(i16::from(x));
                    let y = anchor.1.wrapping_add(i16::from(y));
                    let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
                        continue;
                    };
                    if !self.cell(x, y).is_some_and(|cell| !cell.outside_playfield) {
                        continue;
                    }
                    let cell_index = self.index(x, y).expect("validated real cell");
                    clear_replacement_cell(x, y);
                    stamp_dynamic_tile_identity(
                        &mut self.cells[cell_index],
                        template.tile_id,
                        index as u8,
                        prototype,
                    );
                    self.apply_runtime_lat_slope(x as i16, y as i16, &catalog, &mut changed_cells);
                    for (dx, dy) in [(0i16, -1i16), (1, 0), (0, 1), (-1, 0)] {
                        self.apply_runtime_lat_slope(
                            (x as i16).wrapping_add(dx),
                            (y as i16).wrapping_add(dy),
                            &catalog,
                            &mut changed_cells,
                        );
                    }
                    recalc_dynamic_tile_attributes(&mut self.cells[cell_index], prototype);
                    self.radar_color_valid[cell_index] = true;
                    self.damaged_radar_metadata[cell_index] = None;
                    if !changed_cells.contains(&(x, y)) {
                        changed_cells.push((x, y));
                    }
                }
            }
        }

        let (anim_width, anim_height) = match family {
            DestroyableCliffFamily::A => (5u8, 3u8),
            DestroyableCliffFamily::B => (3u8, 5u8),
        };
        let mut animation_cells = Vec::with_capacity(15);
        for y in 0..anim_height {
            for x in 0..anim_width {
                let x = origin.0.wrapping_add(i16::from(x));
                let y = origin.1.wrapping_add(i16::from(y));
                animation_cells.push((x, y));
            }
        }
        Some(DestroyableCliffMutation {
            #[cfg(any(test, feature = "test-support"))]
            family,
            #[cfg(any(test, feature = "test-support"))]
            origin,
            original_footprint,
            animation_cells,
            changed_cells,
        })
    }

    fn apply_runtime_lat_slope(
        &mut self,
        x: i16,
        y: i16,
        catalog: &DestroyableCliffCatalog,
        changed_cells: &mut Vec<(u16, u16)>,
    ) {
        let (Ok(rx), Ok(ry)) = (u16::try_from(x), u16::try_from(y)) else {
            self.shared_cell_dummy
                .stamp_coord(i32::from(x), i32::from(y));
            return;
        };
        let Some(index) = self.index(rx, ry) else {
            self.shared_cell_dummy
                .stamp_coord(i32::from(x), i32::from(y));
            return;
        };
        if self
            .native_allocated
            .as_ref()
            .is_some_and(|allocated| !allocated.get(index).copied().unwrap_or(false))
            || self.cells[index].outside_playfield
        {
            self.shared_cell_dummy
                .stamp_coord(i32::from(x), i32::from(y));
            return;
        }
        let cardinal_coords = [
            (x, y.wrapping_sub(1)),
            (x.wrapping_add(1), y),
            (x, y.wrapping_add(1)),
            (x.wrapping_sub(1), y),
        ];
        let cardinal_tiles = cardinal_coords.map(|(nx, ny)| {
            u16::try_from(nx)
                .ok()
                .zip(u16::try_from(ny).ok())
                .and_then(|(nx, ny)| self.cell(nx, ny))
                .map_or(0, |cell| match cell.final_tile_index {
                    tile if tile < 0 || tile == 0xFFFF => 0,
                    tile => tile,
                })
        });
        let cardinal_slopes = cardinal_coords.map(|(nx, ny)| {
            u16::try_from(nx)
                .ok()
                .zip(u16::try_from(ny).ok())
                .and_then(|(nx, ny)| self.cell(nx, ny))
                .map_or(0, |cell| cell.slope_type)
        });
        let old_tile = self.cells[index].final_tile_index;
        let lat_tile = lat::lat_fixed_tile(old_tile, cardinal_tiles, &catalog.lat_config);
        let new_tile = lat::slope_fixed_tile(
            lat_tile,
            self.cells[index].slope_type,
            cardinal_slopes,
            catalog.slope_config,
        );
        if new_tile != old_tile {
            self.cells[index].final_tile_index = new_tile;
            if !changed_cells.contains(&(rx, ry)) {
                changed_cells.push((rx, ry));
            }
        }
    }

    /// CellClass lookup used by the collapse animation producer. Invalid
    /// signed coordinates resolve through the one shared dummy; the caller
    /// still retains the signed coordinate for the world X/Y calculation.
    pub(crate) fn collapse_animation_level(&self, x: i16, y: i16) -> i8 {
        let cell = u16::try_from(x)
            .ok()
            .zip(u16::try_from(y).ok())
            .and_then(|(x, y)| self.cell(x, y));
        if let Some(cell) = cell {
            cell.level as i8
        } else {
            self.shared_cell_dummy
                .stamp_coord(i32::from(x), i32::from(y));
            self.dummy_cell_level_slope().0
        }
    }
}

pub(super) fn build_destroyable_cliff_catalog(
    theater_data: Option<&TheaterData>,
    asset_manager: Option<&crate::assets::asset_manager::AssetManager>,
    terrain_rules: Option<&TerrainRules>,
) -> Option<DestroyableCliffCatalog> {
    let td = theater_data?;
    let assets = asset_manager?;
    let destroyable_start = td.cliff_ranges.destroyable_cliffs?;
    let slope_start = td
        .slope_set_pieces
        .and_then(|set| td.lookup.bounds().get(usize::from(set)))?
        .start;
    let mut warned_unknown_land_types = HashSet::new();
    let mut load_template = |tile_id: u16| -> Option<SparseTileTemplate> {
        let filename = td.lookup.filename(i32::from(tile_id))?;
        let bytes = assets.get(filename)?;
        let tmp = TmpFile::from_bytes(&bytes).ok()?;
        let width = u8::try_from(tmp.template_width).ok()?;
        let height = u8::try_from(tmp.template_height).ok()?;
        let entries = tmp
            .tiles
            .iter()
            .enumerate()
            .map(|(sub_tile, entry)| {
                entry.as_ref()?;
                let sub_tile = u8::try_from(sub_tile).ok()?;
                Some(DynamicTilePrototype {
                    metadata: load_tile_metadata(
                        Some(td),
                        Some(assets),
                        terrain_rules,
                        TileKey {
                            tile_id,
                            sub_tile,
                            variant: 0,
                        },
                        &mut warned_unknown_land_types,
                    ),
                    accepts_smudge: td.lookup.is_morphable(tile_id),
                    allows_tiberium: td.lookup.allows_tiberium(tile_id),
                })
            })
            .collect();
        Some(SparseTileTemplate {
            tile_id,
            width,
            height,
            entries,
        })
    };
    Some(DestroyableCliffCatalog {
        destroyable_start,
        old: [
            load_template(destroyable_start)?,
            load_template(destroyable_start.checked_add(1)?)?,
        ],
        replacements: [
            load_template(slope_start)?,
            load_template(slope_start.checked_add(1)?)?,
            load_template(slope_start.checked_add(2)?)?,
            load_template(slope_start.checked_add(3)?)?,
        ],
        lat_config: lat::parse_lat_config(&td.ini_data, &td.lookup),
        slope_config: lat::SlopeFixupConfig {
            ramp_base: td.rmg_tiles.ramp_base.map_or(-1, i32::from),
            ramp_smooth: td.rmg_tiles.ramp_smooth.map_or(-1, i32::from),
        },
    })
}

fn stamp_dynamic_tile_identity(
    cell: &mut ResolvedTerrainCell,
    tile_id: u16,
    sub_tile: u8,
    prototype: &DynamicTilePrototype,
) {
    cell.final_tile_index = i32::from(tile_id);
    cell.final_sub_tile = sub_tile;
    cell.is_wood_bridge_repair_tile = false;
    cell.slope_type = prototype.metadata.slope_type;
    cell.level = (cell.level as i8).wrapping_add(prototype.metadata.template_height as i8) as u8;
    cell.overlay_blocks = false;
    cell.overlay_zone_type = None;
}

fn recalc_dynamic_tile_attributes(
    cell: &mut ResolvedTerrainCell,
    prototype: &DynamicTilePrototype,
) {
    let metadata = &prototype.metadata;
    cell.filled_clear = false;
    cell.tileset_index = metadata.tileset_index;
    cell.land_type = metadata.land_type;
    cell.yr_cell_land_type = metadata.yr_cell_land_type;
    cell.template_height = metadata.template_height;
    cell.height_in_pixels = metadata.height_in_pixels;
    cell.render_offset_x = metadata.render_offset_x;
    cell.render_offset_y = metadata.render_offset_y;
    cell.terrain_class = metadata.terrain_class;
    cell.speed_costs = metadata.speed_costs;
    cell.is_water = metadata.is_water;
    cell.is_cliff_like = metadata.is_cliff_like;
    cell.is_rough = metadata.is_rough;
    cell.is_road = metadata.is_road;
    cell.accepts_smudge = prototype.accepts_smudge;
    cell.allows_tiberium = prototype.allows_tiberium;
    cell.variant = 0;
    cell.has_ramp = metadata.has_ramp;
    cell.canonical_ramp = canonical_ramp_from_slope_type(metadata.slope_type);
    cell.base_ground_walk_blocked = metadata.ground_blocked;
    cell.base_build_blocked = metadata.build_blocked;
    cell.base_land_type = metadata.land_type;
    cell.base_yr_cell_land_type = metadata.yr_cell_land_type;
    cell.base_terrain_class = metadata.terrain_class;
    cell.base_speed_costs = metadata.speed_costs;
    cell.ground_walk_blocked = metadata.ground_blocked || cell.terrain_object_blocks;
    cell.zone_type = recalc_zone_type(
        cell.outside_playfield,
        None,
        cell.land_type,
        cell.speed_costs.wheel,
        cell.terrain_object_occupation,
    );
    cell.has_bridge_deck = false;
    cell.bridge_walkable = false;
    cell.bridge_transition = false;
    cell.bridge_deck_level = cell.level;
    cell.bridge_layer = None;
    cell.bridge_facts = BridgeCellFacts::default();
    cell.radar_left = metadata.radar_left;
    cell.radar_right = metadata.radar_right;
    cell.has_damaged_data = metadata.has_damaged_data;
}
