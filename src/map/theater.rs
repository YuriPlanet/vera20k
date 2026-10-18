//! Theater system: maps tile_id → TMP filenames and loads tile images.
//!
//! Each RA2 map specifies a theater (temperate, snow, urban) which determines
//! the terrain tileset (.tmp files) and palette. Theater INI files define
//! numbered tilesets ([TileSet0000], [TileSet0001], ...) with filename prefix
//! and count. The global tile_id from IsoMapPack5 is a cumulative index.
//!
//! ## Dependency rules
//! - Part of map/ — depends on assets/, rules/ (for INI parsing).
//! - Does NOT depend on render/ or sim/.

use std::collections::{HashMap, HashSet};

use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::{Color, Palette};
use crate::assets::tmp_file::TmpFile;
use crate::map::map_file::MapError;
use crate::rules::ini_parser::{IniFile, IniSection};

/// Marker for "no tile" after raw Pack5 compatibility translation.
///
/// A raw signed `-1` can be shifted by a reached `LastTilesInSet` record, so
/// callers must not classify it before the map-load translation seam. Positive
/// legacy u16 `0xFFFF` is translation-exempt and is also treated as no-tile.
pub const NO_TILE: i32 = -1;

/// Identifies a specific sub-tile within a TMP template, including variant.
/// Used as a key for atlas lookups. Variant 0 is pristine; positive values
/// index the theater-resolved contiguous suffix chain.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub struct TileKey {
    pub tile_id: u16,
    pub sub_tile: u8,
    /// Visual replacement index: 0 = pristine, 1 = `a`, 2 = `b`, etc.
    pub variant: u8,
}

/// RGBA pixel data for a single rendered tile, before atlas packing.
///
/// For tiles with extra data (cliff faces, shores), width/height include the
/// extra region. offset_x/offset_y indicate where the standard 60×30 diamond
/// origin sits within this enlarged buffer.
pub struct TileImage {
    pub rgba: Vec<u8>,
    /// Per-pixel Z-depth from TMP file (same dimensions as rgba, one byte per pixel).
    /// Added to the native tile base Z for pixel admission and storage.
    /// Stock flat Clear01 diamonds also carry nonzero values (1..29).
    pub depth: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// X offset of diamond origin within this buffer (negative = extra data extends left).
    pub offset_x: i32,
    /// Y offset of diamond origin within this buffer (negative = extra data extends above).
    pub offset_y: i32,
}

/// Static definition for a theater.
struct TheaterDef {
    /// Exact active-YR theater INI filename.
    ini_name: &'static str,
    /// File extension for TMP files (e.g., "tem" for temperate).
    extension: &'static str,
    /// Exact isometric, unit, and theater/ore palette filenames.
    iso_palette_name: &'static str,
    unit_palette_name: &'static str,
    theater_palette_name: &'static str,
    /// Theater-specific MIX archives in retail construction order.
    mix_archives: &'static [&'static str],
}

/// All active Yuri's Revenge theater definitions.
const THEATER_DEFS: &[(&str, TheaterDef)] = &[
    (
        "TEMPERATE",
        TheaterDef {
            ini_name: "temperatmd.ini",
            extension: "tem",
            iso_palette_name: "isotem.pal",
            unit_palette_name: "unittem.pal",
            theater_palette_name: "temperat.pal",
            mix_archives: &["temperat.mix", "tem.mix", "isotemmd.mix", "isotemp.mix"],
        },
    ),
    (
        "SNOW",
        TheaterDef {
            ini_name: "snowmd.ini",
            extension: "sno",
            iso_palette_name: "isosno.pal",
            unit_palette_name: "unitsno.pal",
            theater_palette_name: "snow.pal",
            mix_archives: &[
                "snowmd.mix",
                "snow.mix",
                "sno.mix",
                "isosnomd.mix",
                "isosnow.mix",
            ],
        },
    ),
    (
        "URBAN",
        TheaterDef {
            ini_name: "urbanmd.ini",
            extension: "urb",
            iso_palette_name: "isourb.pal",
            unit_palette_name: "uniturb.pal",
            theater_palette_name: "urban.pal",
            mix_archives: &["urban.mix", "urb.mix", "isourbmd.mix", "isourb.mix"],
        },
    ),
    (
        "LUNAR",
        TheaterDef {
            ini_name: "lunarmd.ini",
            extension: "lun",
            iso_palette_name: "isolun.pal",
            unit_palette_name: "unitlun.pal",
            theater_palette_name: "lunar.pal",
            mix_archives: &["lunar.mix", "lun.mix", "isolunmd.mix", "isolun.mix"],
        },
    ),
    (
        "DESERT",
        TheaterDef {
            ini_name: "desertmd.ini",
            extension: "des",
            iso_palette_name: "isodes.pal",
            unit_palette_name: "unitdes.pal",
            theater_palette_name: "desert.pal",
            mix_archives: &["desert.mix", "des.mix", "isodesmd.mix", "isodes.mix"],
        },
    ),
    (
        "NEWURBAN",
        TheaterDef {
            ini_name: "urbannmd.ini",
            extension: "ubn",
            iso_palette_name: "isoubn.pal",
            unit_palette_name: "unitubn.pal",
            theater_palette_name: "urbann.pal",
            mix_archives: &["urbann.mix", "ubn.mix", "isoubnmd.mix", "isoubn.mix"],
        },
    ),
];

/// Start tile_id and count for one tileset section (e.g., [TileSet0013]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TilesetBounds {
    /// First tile_id belonging to this tileset.
    pub start: u16,
    /// Number of tiles in this tileset.
    pub count: u16,
}

/// Numeric theater cliff/ramp ranges loaded from `[General]`.
///
/// Gamemd.exe reads these keys as tileset ordinals, then stores the cumulative
/// tile_id start for each matching tileset. The classifiers intentionally use
/// fixed retail range lengths rather than `TilesInSet`, because several stock
/// ranges span multiple adjacent tilesets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TheaterCliffRanges {
    pub cliff_set: Option<u16>,
    pub cliff_ramps: Option<u16>,
    pub water_cliffs: Option<u16>,
    pub destroyable_cliffs: Option<u16>,
    pub bridge_set: Option<u16>,
    pub wood_bridge_set: Option<u16>,
    pub water_caves: Option<u16>,
    pub waterfall_east: Option<u16>,
    pub waterfall_west: Option<u16>,
    pub waterfall_north: Option<u16>,
    pub waterfall_south: Option<u16>,
}

impl TheaterCliffRanges {
    /// Broad special-terrain identity predicate used by terrain-height logic.
    pub fn is_special_terrain_tile(&self, tile_id: u16, sub_tile: u8) -> bool {
        in_fixed_range(self.cliff_set, tile_id, 0x28)
            || in_fixed_range(self.cliff_ramps, tile_id, 0x14)
            || in_fixed_range(self.water_cliffs, tile_id, 0x1c)
            || in_fixed_range(self.destroyable_cliffs, tile_id, 2)
            || in_fixed_range(self.bridge_set, tile_id, 0x10)
            || in_fixed_range(self.wood_bridge_set, tile_id, 0x10)
            || in_fixed_range(self.water_caves, tile_id, 4)
            || waterfall_is_special(self.waterfall_east, tile_id, sub_tile, &[0, 4])
            || waterfall_is_special(self.waterfall_west, tile_id, sub_tile, &[1, 3])
            || waterfall_is_special(self.waterfall_south, tile_id, sub_tile, &[0, 1])
            || waterfall_is_special(self.waterfall_north, tile_id, sub_tile, &[2, 3])
    }

    /// Narrow `IsOnBridgeRamp @ 0x00578d80` predicate.
    pub fn is_on_bridge_ramp_tile(&self, tile_id: u16, slope_byte: u8) -> bool {
        in_fixed_range(self.cliff_set, tile_id, 0x28)
            || in_fixed_range(self.cliff_ramps, tile_id, 0x14)
            || waterfall_blocks(self.waterfall_east, tile_id, slope_byte, &[0, 4])
            || waterfall_blocks(self.waterfall_west, tile_id, slope_byte, &[1, 3])
            || waterfall_blocks(self.waterfall_south, tile_id, slope_byte, &[0, 1])
            || waterfall_blocks(self.waterfall_north, tile_id, slope_byte, &[2, 3])
    }
}

/// Theater `[General]` tile-identity keys the random-map generator resolves
/// to flat tile ids at load, following the same ordinal→first-tile scheme as
/// the cliff ranges above. A missing key stays `None` (the original's globals
/// default to -1, so absence must stay distinguishable from tile 0).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RmgTileKeys {
    pub clear_tile: Option<u16>,
    pub ramp_base: Option<u16>,
    pub ramp_smooth: Option<u16>,
    /// Resolved ABC1F8: SlopeSetPieces read545776, published545E26.
    /// Ramp-end blocks use this family independently of RampBase/CliffRamps.
    pub slope_set_pieces: Option<u16>,
    pub rough_tile: Option<u16>,
    pub sand_tile: Option<u16>,
    pub green_tile: Option<u16>,
    pub clear_to_rough_lat: Option<u16>,
    pub clear_to_sand_lat: Option<u16>,
    pub clear_to_green_lat: Option<u16>,
    pub clear_to_pave_lat: Option<u16>,
    pub pave_tile: Option<u16>,
    pub water_set: Option<u16>,
    pub shore_pieces: Option<u16>,
    pub water_bridge: Option<u16>,
    pub misc_pave_tile: Option<u16>,
    pub paved_roads: Option<u16>,
    pub paved_road_ends: Option<u16>,
    pub medians: Option<u16>,
}

fn in_fixed_range(start: Option<u16>, tile_id: u16, len: u16) -> bool {
    let Some(start) = start else {
        return false;
    };
    let end = start.saturating_add(len);
    tile_id >= start && tile_id < end
}

fn waterfall_blocks(start: Option<u16>, tile_id: u16, slope_byte: u8, passable: &[u8]) -> bool {
    let Some(start) = start else {
        return false;
    };
    if !in_fixed_range(Some(start), tile_id, 4) {
        return false;
    }
    let offset = tile_id - start;
    if offset == 0 || offset == 3 {
        !passable.contains(&slope_byte)
    } else {
        true
    }
}

/// A `Tile%02dAnim` block: one terrain-attached animation declared for a single
/// tile of a tileset.
///
/// The theater loader reads these from the INI section named by the tileset's
/// `SetName` value — NOT from `[TileSetNNNN]`. Stock example: `[TileSet0049]`
/// carries `SetName=Waterfalls-B`, and its animation keys live in the separate
/// `[Waterfalls-B]` section. `%02d` is the 1-based tile ordinal inside the set.
///
/// The animation's own cadence (frame count, `Rate`, `LoopStart`/`LoopEnd`,
/// `LoopCount`) is NOT part of this block — it comes from the named AnimType's
/// `art(md).ini` section, exactly like every other animation in the game. This
/// block only says *which* animation, *where*, and with what sort bias.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileAnimAttachment {
    /// `Tile%02dAnim` — AnimType id. Present only when the key parsed non-empty.
    pub anim_name: String,
    /// `Tile%02dXOffset` — screen-pixel offset from the cell centre.
    pub x_offset: i32,
    /// `Tile%02dYOffset` — screen-pixel offset from the cell centre.
    pub y_offset: i32,
    /// `Tile%02dAttachesTo` — the sub-tile index that spawns the animation.
    /// Defaults to `TILE_ANIM_NO_SUBTILE`, which matches no sub-tile, so a
    /// block without this key never spawns.
    pub attaches_to: i32,
    /// `Tile%02dZAdjust` — sort bias handed to the spawned animation.
    pub z_adjust: i32,
}

/// `AttachesTo` sentinel when the key is absent. The tile type constructor
/// leaves this field at -1 and the spawn gate compares it for equality against
/// an unsigned sub-tile index, so -1 can never match.
pub const TILE_ANIM_NO_SUBTILE: i32 = -1;
/// `XOffset` / `YOffset` / `ZAdjust` all start at 0 in the tile type constructor
/// and the INI read passes the current value as its own default.
const TILE_ANIM_DEFAULT_OFFSET: i32 = 0;
const TILE_ANIM_DEFAULT_Z_ADJUST: i32 = 0;

/// Maps tile_id → TMP filename. Built by parsing a theater INI file.
pub struct TilesetLookup {
    /// tile_id → TMP filename (e.g., "clear01.tem"). None = blank/empty tileset.
    entries: Vec<Option<String>>,
    /// tile_id → variant TMP filenames (e.g., ["clear01a.tem", "clear01b.tem"]).
    /// Existing suffix siblings are discovered after theater archive activation.
    /// Empty means that this tile has only its pristine file.
    variant_filenames: Vec<Vec<String>>,
    /// Tileset index → bounds (start tile_id and count).
    /// Index 0 corresponds to [TileSet0000], etc.
    tileset_bounds: Vec<TilesetBounds>,
    /// Tileset index → SetName from theater INI (e.g., "Water", "Cliffs", "Grass").
    /// Used to classify tiles for walkability (water/cliff detection).
    set_names: Vec<String>,
    /// Per-tileset Morphable= flag — parsed from `[TileSetNNNN] Morphable=`.
    /// Default `false`. Smudges only place on cells whose tileset is morphable.
    morphable_flags: Vec<bool>,
    /// Per-tileset AllowTiberium= flag — parsed from `[TileSetNNNN] AllowTiberium=`.
    /// Default `false`. Tiberium/ore only places on opt-in in-range tile types.
    allow_tiberium_flags: Vec<bool>,
    /// tile_id → terrain animation attachment, parallel to `entries`.
    /// `None` means the tile declared no `Tile%02dAnim`, matching the tile
    /// type's -1 animation index.
    tile_anims: Vec<Option<TileAnimAttachment>>,
    /// Declaration-ordered compatibility records built from each unequal
    /// `LastTilesInSet`. Values replace native's pointer-vector storage while
    /// retaining its exact signed walk semantics.
    legacy_tile_index_exceptions: Vec<LegacyTileIndexException>,
}

/// Immutable value projection of one native separately allocated eight-byte
/// `LegacyException` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LegacyTileIndexException {
    legacy_boundary: i32,
    delta: i32,
}

const MAX_USABLE_TILE_SLOTS: usize = u16::MAX as usize;
const MAX_TILESET_ORDINAL: u32 = u16::MAX as u32;

impl TilesetLookup {
    /// Get the TMP filename for a given tile index.
    /// Returns None for NO_TILE (-1), out-of-range, or blank tilesets.
    pub fn filename(&self, tile_index: i32) -> Option<&str> {
        if tile_index < 0 {
            return None;
        }
        self.entries
            .get(tile_index as usize)
            .and_then(|opt| opt.as_deref())
    }

    /// Total number of tile_id slots (including blanks).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get the tileset bounds table (tileset_index → start, count).
    pub fn bounds(&self) -> &[TilesetBounds] {
        &self.tileset_bounds
    }

    /// Find which tileset index a tile_id belongs to.
    /// Returns None for negative or out-of-range tile IDs.
    pub fn tileset_index(&self, tile_id: u16) -> Option<u16> {
        for (idx, b) in self.tileset_bounds.iter().enumerate() {
            if tile_id >= b.start && tile_id < b.start + b.count {
                return Some(idx as u16);
            }
        }
        None
    }

    /// Get the SetName for a tileset index (e.g., "Rough", "Cliff Set").
    pub fn set_name(&self, tileset_idx: u16) -> Option<&str> {
        self.set_names.get(tileset_idx as usize).map(|s| s.as_str())
    }

    /// Number of suffix siblings for a tile_id (0 = pristine only).
    pub fn variant_count(&self, tile_id: u16) -> u8 {
        self.variant_filenames
            .get(tile_id as usize)
            .map(|v| v.len() as u8)
            .unwrap_or(0)
    }

    /// Total independent TMP file count, including the pristine file.
    pub fn total_file_count(&self, tile_id: u16) -> u8 {
        self.filename(i32::from(tile_id))
            .map(|_| self.variant_count(tile_id).saturating_add(1))
            .unwrap_or(0)
    }

    /// Get the variant TMP filenames for a tile_id (may be empty).
    pub fn variant_filenames(&self, tile_id: u16) -> &[String] {
        self.variant_filenames
            .get(tile_id as usize)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Exact independent TMP filename for a resolved file index.
    pub fn filename_for_variant(&self, tile_id: u16, variant: u8) -> Option<&str> {
        if variant == 0 {
            self.filename(i32::from(tile_id))
        } else {
            self.variant_filenames(tile_id)
                .get(usize::from(variant - 1))
                .map(String::as_str)
        }
    }

    /// Returns true if a tile_id belongs to a tileset with `Morphable=yes`.
    /// Smudges (craters, scorches) only place on morphable tiles.
    pub fn is_morphable(&self, tile_id: u16) -> bool {
        let idx: u16 = match self.tileset_index(tile_id) {
            Some(i) => i,
            None => return false,
        };
        self.morphable_flags
            .get(idx as usize)
            .copied()
            .unwrap_or(false)
    }

    /// Terrain animation attached to a tile_id, if its tileset declared one.
    pub fn tile_anim(&self, tile_id: u16) -> Option<&TileAnimAttachment> {
        self.tile_anims.get(tile_id as usize)?.as_ref()
    }

    /// Returns true if a tile_id belongs to a tileset with `AllowTiberium=yes`.
    /// Absent keys, missing tilesets, and out-of-range tile ids default false.
    pub fn allows_tiberium(&self, tile_id: u16) -> bool {
        let idx: u16 = match self.tileset_index(tile_id) {
            Some(i) => i,
            None => return false,
        };
        self.allow_tiberium_flags
            .get(idx as usize)
            .copied()
            .unwrap_or(false)
    }

    /// Translate one raw IsoMap tile index through the active theater's
    /// declaration-ordered `LastTilesInSet` compatibility records.
    ///
    /// Retail provenance: `CalculateLegacyMapTileIndex @ 0x00544E30` keeps
    /// positive 0xFFFF unchanged, compares every signed boundary with the
    /// original raw input, stops on the first greater boundary, and wrapping-
    /// adds every delta already reached.
    pub fn translate_legacy_map_tile_index(&self, raw: i32) -> i32 {
        if raw == i32::from(u16::MAX) {
            return raw;
        }
        let mut result = raw;
        for entry in &self.legacy_tile_index_exceptions {
            if entry.legacy_boundary > raw {
                break;
            }
            result = result.wrapping_add(entry.delta);
        }
        result
    }
}

fn checked_tileset_ordinal(ordinal: u32) -> Result<u16, MapError> {
    u16::try_from(ordinal).map_err(|_| MapError::TilesetOrdinalOverflow {
        ordinal,
        maximum: MAX_TILESET_ORDINAL,
    })
}

fn read_tileset_row(ini: &IniFile, ordinal: u32) -> Result<Option<(&IniSection, i32)>, MapError> {
    let section_name = format!("TileSet{ordinal:04}");
    let section = ini.section(&section_name);
    let tiles_in_set = section.map_or(-1, |section| section.read_int("TilesInSet", -1));
    if tiles_in_set == -1 {
        return Ok(None);
    }
    checked_tileset_ordinal(ordinal)?;
    Ok(Some((
        section.expect("nonterminating TilesInSet requires a section"),
        tiles_in_set,
    )))
}

/// Parse a theater INI file into a TilesetLookup.
///
/// Iterates [TileSet0000], [TileSet0001], ... sections in order.
/// Each section's positive `TilesInSet` advances the global tile_id counter.
/// The filename is `{FileName}{NN:02}.{extension}` where NN is 1-indexed.
/// Blank FileName entries consume tile_id slots but map to None.
pub fn parse_tileset_ini(ini_data: &[u8], extension: &str) -> Result<TilesetLookup, MapError> {
    let ini = IniFile::from_bytes(ini_data).map_err(MapError::Ini)?;
    parse_tileset_sections(&ini, extension)
}

fn parse_tileset_sections(ini: &IniFile, extension: &str) -> Result<TilesetLookup, MapError> {
    let mut entries: Vec<Option<String>> = Vec::new();
    let mut variant_filenames: Vec<Vec<String>> = Vec::new();
    let mut tileset_bounds: Vec<TilesetBounds> = Vec::new();
    let mut set_names: Vec<String> = Vec::new();
    let mut morphable_flags: Vec<bool> = Vec::new();
    let mut allow_tiberium_flags: Vec<bool> = Vec::new();
    let mut tile_anims: Vec<Option<TileAnimAttachment>> = Vec::new();
    let mut legacy_tile_index_exceptions = Vec::new();
    let mut actual_cursor = 0i32;
    let mut legacy_cursor = 0i32;

    // Retail provenance: `Read_Theater_TileSets_INI @ 0x00545150` publishes
    // candidates in ordinal order and terminates only when ReadInt returns the
    // exact signed -1 sentinel. `%04d` is a minimum width, not a 10,000-row cap.
    let mut idx = 0u32;
    while let Some((section, tiles_in_set)) = read_tileset_row(ini, idx)? {
        // 0x40-byte ReadStrings (`0x005460C0`, `0x005460E5`). An absent
        // `FileName` reads the native default `TILE`; Rust keeps that set
        // blank instead (every retail tileset names its file).
        let filename: &str = section.read_name("FileName", 0x40).unwrap_or("");
        let set_name: &str = section.read_name("SetName", 0x40).unwrap_or("No Name");
        let last_tiles_in_set = section.read_int("LastTilesInSet", -1);
        if last_tiles_in_set != -1 && last_tiles_in_set != tiles_in_set {
            let boundary = legacy_cursor.wrapping_add(last_tiles_in_set);
            legacy_tile_index_exceptions.push(LegacyTileIndexException {
                legacy_boundary: boundary,
                delta: tiles_in_set.wrapping_sub(last_tiles_in_set),
            });
            legacy_cursor = boundary;
        } else {
            legacy_cursor = legacy_cursor.wrapping_add(tiles_in_set);
        }

        // Native creates no tile objects for zero or negative counts other
        // than the terminating -1, but its compatibility cursor above still
        // advances. Positive counts retain missing/blank registry slots.
        let represented_count = usize::try_from(tiles_in_set).unwrap_or(0);
        let attempted_slots = entries.len().saturating_add(represented_count);
        if attempted_slots > MAX_USABLE_TILE_SLOTS {
            return Err(MapError::TilesetRegistryTooLarge {
                attempted: attempted_slots,
                maximum: MAX_USABLE_TILE_SLOTS,
            });
        }
        let tiles_in_set = u16::try_from(represented_count)
            .expect("registry limit proves each represented count fits u16");

        let start = u16::try_from(actual_cursor)
            .expect("safe registry domain proves the native actual cursor fits u16");
        debug_assert_eq!(usize::from(start), entries.len());
        tileset_bounds.push(TilesetBounds {
            start,
            count: tiles_in_set,
        });
        set_names.push(set_name.to_string());
        let morphable: bool = section.read_bool("Morphable", false);
        morphable_flags.push(morphable);
        let allow_tiberium: bool = section.read_bool("AllowTiberium", false);
        allow_tiberium_flags.push(allow_tiberium);

        // Diagnostic: log ALL tileset TilesInSet values for debugging.
        log::debug!(
            "  TileSet{:04} TilesInSet={} start={} file={} name={}",
            idx,
            tiles_in_set,
            start,
            filename,
            set_name
        );

        // Animation keys live in the section named by the effective SetName.
        // A missing key uses the native "No Name" default and can therefore
        // resolve a `[No Name]` section; only an empty name or an absent named
        // section has no animation block. Resolve it once before the per-tile
        // walk.
        let anim_section: Option<&IniSection> = if set_name.is_empty() {
            None
        } else {
            ini.section(set_name)
        };

        if filename.is_empty() {
            // Blank tileset — consume slots but produce None entries.
            for _ in 0..usize::from(tiles_in_set) {
                entries.push(None);
                variant_filenames.push(Vec::new());
                tile_anims.push(None);
            }
        } else {
            // Each tile is named {prefix}{NN:02}.{ext}, 1-indexed. Sibling
            // discovery waits until load_theater has activated the theater
            // archives, keeping this parser asset-independent.
            for i in 1..=u32::from(tiles_in_set) {
                let main_name = format!("{}{:02}.{}", filename, i, extension);
                entries.push(Some(main_name));
                variant_filenames.push(Vec::new());
                tile_anims.push(anim_section.and_then(|section| parse_tile_anim(section, i)));
            }
        }
        actual_cursor = actual_cursor.wrapping_add(i32::from(tiles_in_set));
        idx = idx.checked_add(1).ok_or(MapError::TilesetOrdinalOverflow {
            ordinal: u32::MAX,
            maximum: MAX_TILESET_ORDINAL,
        })?;
    }

    // Diagnostic: log first 15 tilesets for debugging tile mapping.
    for (idx, (bounds, name)) in tileset_bounds
        .iter()
        .zip(set_names.iter())
        .enumerate()
        .take(15)
    {
        let fname: &str = entries
            .get(bounds.start as usize)
            .and_then(|o| o.as_deref())
            .unwrap_or("(blank)");
        log::info!(
            "  TileSet{:04}: start={:4}, count={:3}, name={:20} file={}",
            idx,
            bounds.start,
            bounds.count,
            name,
            fname,
        );
    }
    log::info!(
        "  ... {} total tilesets, {} total tile_id slots",
        tileset_bounds.len(),
        entries.len()
    );

    Ok(TilesetLookup {
        entries,
        variant_filenames,
        tileset_bounds,
        set_names,
        morphable_flags,
        allow_tiberium_flags,
        tile_anims,
        legacy_tile_index_exceptions,
    })
}

/// Read one tile's animation block out of a `SetName`-named section.
///
/// `tile_ordinal` is 1-based within the tileset, matching the `%02d` the loader
/// formats into every key. The whole block is gated on `Tile%02dAnim` reading
/// back a non-empty string: the loader only stores the offsets, `AttachesTo`
/// and `ZAdjust` after the animation name resolved to an AnimType. All four
/// numeric keys pass the field's current value as their own INI default, which
/// for a freshly constructed tile type is 0 / 0 / -1 / 0.
fn parse_tile_anim(section: &IniSection, tile_ordinal: u32) -> Option<TileAnimAttachment> {
    // 0x80-byte ReadString at `0x00546524`, then `AnimTypeClass::Find`.
    let anim_name = section.read_name(&format!("Tile{:02}Anim", tile_ordinal), 0x80)?;
    Some(TileAnimAttachment {
        anim_name: anim_name.to_string(),
        x_offset: section.read_int(
            &format!("Tile{:02}XOffset", tile_ordinal),
            TILE_ANIM_DEFAULT_OFFSET,
        ),
        y_offset: section.read_int(
            &format!("Tile{:02}YOffset", tile_ordinal),
            TILE_ANIM_DEFAULT_OFFSET,
        ),
        attaches_to: section.read_int(
            &format!("Tile{:02}AttachesTo", tile_ordinal),
            TILE_ANIM_NO_SUBTILE,
        ),
        z_adjust: section.read_int(
            &format!("Tile{:02}ZAdjust", tile_ordinal),
            TILE_ANIM_DEFAULT_Z_ADJUST,
        ),
    })
}

/// The TMP extension of a theater name (e.g. "TEMPERATE" -> "tem").
pub fn theater_extension(name: &str) -> Option<&'static str> {
    theater_def(name).map(|def| def.extension)
}

/// Look up the theater definition for a theater name (e.g., "TEMPERATE").
fn theater_def(name: &str) -> Option<&'static TheaterDef> {
    let upper: String = name.to_ascii_uppercase();
    THEATER_DEFS
        .iter()
        .find(|(n, _)| *n == upper)
        .map(|(_, def)| def)
}

/// Result of loading theater data: tilesets, palettes, extension, raw INI bytes.
pub struct TheaterData {
    pub lookup: TilesetLookup,
    /// Isometric terrain palette (for tile rendering).
    pub iso_palette: Palette,
    /// Unit palette (for sprites on this theater).
    pub unit_palette: Palette,
    /// Tiberium palette (for ore/gem overlays). Falls back to iso_palette if not found.
    pub tiberium_palette: Palette,
    /// File extension for TMP files (e.g., "tem").
    pub extension: &'static str,
    /// Raw INI bytes (needed by LAT config parsing).
    pub ini_data: Vec<u8>,
    /// Tileset index for concrete/stone bridgehead tiles (BridgeSet= in theater INI).
    pub bridge_set: Option<u16>,
    /// Tileset index for wooden bridgehead tiles (WoodBridgeSet= in theater INI).
    pub wood_bridge_set: Option<u16>,
    /// `[General] SlopeSetPieces=N` - TileSet section whose first tile becomes DAT_00ABC1F8.
    pub slope_set_pieces: Option<u16>,
    /// `[General] SlopeSetPieces2=N` - TileSet section whose first tile becomes DAT_00AA1098.
    pub slope_set_pieces2: Option<u16>,
    /// `[General] BridgeMiddle1=N` — BridgeSet-relative offset for the NS
    /// bridgehead variant block. The 4 NS variant tile_ids occupy
    /// `BridgeSet_start + {N-1, N, N+1, N+2}`. None if the key is absent.
    pub bridge_middle_1: Option<u16>,
    /// `[General] BridgeMiddle2=N` — same for EW.
    pub bridge_middle_2: Option<u16>,
    /// `[General] Tunnels=N` - theater tile set for tunnel/low-bridge tube cells.
    pub tunnels: Option<u16>,
    /// `[General] TrackTunnels=N` - track tunnel tile set.
    pub track_tunnels: Option<u16>,
    /// `[General] DirtTunnels=N` - dirt tunnel tile set.
    pub dirt_tunnels: Option<u16>,
    /// `[General] DirtTrackTunnels=N` - dirt track tunnel tile set.
    pub dirt_track_tunnels: Option<u16>,
    /// Exact signed cumulative bases published for the four automatic Tube
    /// families in native comparison order. Missing or unresolved keys retain
    /// `-1`; zero-count TileSets retain their aliased cumulative base.
    pub automatic_tube_bases: [i32; 4],
    /// Numeric cliff/ramp/waterfall classifiers derived from theater `[General]`.
    pub cliff_ranges: TheaterCliffRanges,
    /// Tile-identity `[General]` keys consumed by the random-map generator.
    pub rmg_tiles: RmgTileKeys,
}

/// Every BridgeMiddle1/2 tile a live bridge publication can write, for the
/// tile atlas pre-load. Relative to each family's tileset base (BridgeSet,
/// WoodBridgeSet), a middle variant `v` is `base + key - 1 + v`: damage
/// floods write variant 3 and collapse floods and ramp tails variant 4
/// (576BA0/571490 via 56EB80; 572230..573170 and their wooden twins).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeMiddleTiles {
    pub tile_ids: Vec<u16>,
}

impl TheaterData {
    pub fn is_special_terrain_tile(&self, tile_id: u16, sub_tile: u8) -> bool {
        self.cliff_ranges.is_special_terrain_tile(tile_id, sub_tile)
    }

    pub fn is_on_bridge_ramp_tile(&self, tile_id: u16, slope_byte: u8) -> bool {
        self.cliff_ranges
            .is_on_bridge_ramp_tile(tile_id, slope_byte)
    }
}

impl BridgeMiddleTiles {
    /// None without BridgeMiddle1/2 or without either family's tileset.
    /// Variants past the end of the tileset lookup are omitted.
    pub fn from_theater(td: &TheaterData) -> Option<Self> {
        let keys = [td.bridge_middle_1?, td.bridge_middle_2?];
        let max_tile = td.lookup.len() as u32;
        let mut tile_ids = Vec::new();
        for set in [td.bridge_set, td.wood_bridge_set].into_iter().flatten() {
            let Some(start) = td.lookup.bounds().get(usize::from(set)).map(|b| b.start) else {
                continue;
            };
            for key in keys.into_iter().filter(|&key| key >= 1) {
                for variant in 0..=4 {
                    let tile = u32::from(start) + u32::from(key) - 1 + variant;
                    if tile < max_tile && !tile_ids.contains(&(tile as u16)) {
                        tile_ids.push(tile as u16);
                    }
                }
            }
        }
        (!tile_ids.is_empty()).then_some(Self { tile_ids })
    }
}

/// Load tileset data for a theater.
///
/// Replaces the active theater MIX group, then loads the exact active-YR INI
/// and palette filenames.
/// The AssetManager is mutable because theater MIX archives are loaded on demand.
pub fn load_theater(asset_manager: &mut AssetManager, theater_name: &str) -> Option<TheaterData> {
    let def: &TheaterDef = theater_def(theater_name)?;

    if let Err(err) = asset_manager.activate_theater_archives(theater_name, def.mix_archives) {
        log::warn!(
            "Theater {}: archive activation failed: {}",
            theater_name,
            err
        );
        return None;
    }

    let (ini_data, ini_source) = asset_manager.get_with_source(def.ini_name)?;
    log::info!(
        "Theater {}: INI '{}' from {}",
        theater_name,
        def.ini_name,
        ini_source
    );

    // TileSet rows and General metadata share the native INI parser. Original
    // 545535..545C3F uses exact-case General keys and ReadInt5276D0 syntax;
    // tools/rules_oracle/theater_general_reader.md preserves executed results.
    let ini = IniFile::from_bytes(&ini_data).ok()?;
    let mut lookup = parse_tileset_sections(&ini, def.extension).ok()?;
    resolve_contiguous_variant_chains(&mut lookup, asset_manager);
    log::info!(
        "Theater {}: loaded {} from INI '{}' ({} tile_id slots, {} tilesets)",
        theater_name,
        def.extension,
        def.ini_name,
        lookup.len(),
        lookup.bounds().len()
    );

    let iso_palette = load_exact_palette(asset_manager, def.iso_palette_name, theater_name, "iso")?;
    let unit_palette =
        load_exact_palette(asset_manager, def.unit_palette_name, theater_name, "unit")?;
    let tiberium_palette = load_exact_palette(
        asset_manager,
        def.theater_palette_name,
        theater_name,
        "theater",
    )
    .unwrap_or_else(native_missing_theater_palette);

    // Original ReadTheater545150 reads these from the same General section
    // as the signed repair/rim owner. These u16 fields are bounded tile-index
    // projections; HighBridgeRimTiles independently retains signed values.
    let general = ini.section("General");
    let mut bridge_set = read_general_u16(general, "BridgeSet");
    let mut wood_bridge_set = read_general_u16(general, "WoodBridgeSet");
    let slope_set_pieces = read_general_u16(general, "SlopeSetPieces");
    let slope_set_pieces2 = read_general_u16(general, "SlopeSetPieces2");
    let bridge_middle_1 = read_general_u16(general, "BridgeMiddle1");
    let bridge_middle_2 = read_general_u16(general, "BridgeMiddle2");
    let tunnels = read_general_u16(general, "Tunnels");
    let track_tunnels = read_general_u16(general, "TrackTunnels");
    let dirt_tunnels = read_general_u16(general, "DirtTunnels");
    let dirt_track_tunnels = read_general_u16(general, "DirtTrackTunnels");
    let automatic_tube_bases =
        [tunnels, track_tunnels, dirt_tunnels, dirt_track_tunnels].map(|ordinal| {
            ordinal
                .and_then(|ordinal| lookup.bounds().get(ordinal as usize))
                .map_or(-1, |bounds| i32::from(bounds.start))
        });
    let mut cliff_ranges = resolve_cliff_ranges(&lookup, general, bridge_set, wood_bridge_set);
    let mut rmg_tiles = resolve_rmg_tile_keys(&lookup, general, slope_set_pieces);
    apply_lunar_global_zeroing(
        theater_name,
        &mut bridge_set,
        &mut wood_bridge_set,
        &mut cliff_ranges,
        &mut rmg_tiles,
    );
    if bridge_set.is_some() || wood_bridge_set.is_some() {
        log::info!(
            "Theater {}: BridgeSet={:?}, WoodBridgeSet={:?}, BridgeMiddle={:?}/{:?}, Tunnels={:?}/{:?}/{:?}/{:?}",
            theater_name,
            bridge_set,
            wood_bridge_set,
            bridge_middle_1,
            bridge_middle_2,
            tunnels,
            track_tunnels,
            dirt_tunnels,
            dirt_track_tunnels,
        );
    }
    // Diagnostic: bridge anchor variant rendering needs both BridgeSet
    // AND BridgeMiddle1/2. Log once at theater load if any are missing so
    // mods without these keys produce a clear single warning rather than
    // silent per-frame visual drift.
    if bridge_set.is_some() && (bridge_middle_1.is_none() || bridge_middle_2.is_none()) {
        log::info!(
            "Theater {}: BridgeSet present but BridgeMiddle{} missing — bridgehead anchor damage visuals disabled",
            theater_name,
            match (bridge_middle_1, bridge_middle_2) {
                (None, None) => "1+2",
                (None, _) => "1",
                _ => "2",
            },
        );
    }

    Some(TheaterData {
        lookup,
        iso_palette,
        unit_palette,
        tiberium_palette,
        extension: def.extension,
        ini_data,
        bridge_set,
        wood_bridge_set,
        slope_set_pieces,
        slope_set_pieces2,
        bridge_middle_1,
        bridge_middle_2,
        tunnels,
        track_tunnels,
        dirt_tunnels,
        dirt_track_tunnels,
        automatic_tube_bases,
        cliff_ranges,
        rmg_tiles,
    })
}

/// The stock corpus stops at `g`. The alphabet-wide cap is deliberately well
/// beyond retail while keeping defensive filename generation bounded for mods.
const MAX_VARIANT_SUFFIXES: usize = 26;

fn contiguous_variant_filenames(
    pristine: &str,
    mut exists: impl FnMut(&str) -> bool,
) -> Vec<String> {
    if !exists(pristine) {
        return Vec::new();
    }
    let Some((stem, extension)) = pristine.rsplit_once('.') else {
        return Vec::new();
    };
    let mut siblings = Vec::new();
    for suffix_offset in 0..MAX_VARIANT_SUFFIXES {
        let suffix = char::from(b'a' + suffix_offset as u8);
        let candidate = format!("{stem}{suffix}.{extension}");
        if !exists(&candidate) {
            break;
        }
        siblings.push(candidate);
    }
    siblings
}

fn resolve_contiguous_variant_chains(lookup: &mut TilesetLookup, asset_manager: &AssetManager) {
    let mut sibling_files = 0usize;
    let mut groups = 0usize;
    let mut max_total_files = 1usize;
    for (pristine, siblings) in lookup
        .entries
        .iter()
        .zip(lookup.variant_filenames.iter_mut())
    {
        let Some(pristine) = pristine.as_deref() else {
            continue;
        };
        *siblings = contiguous_variant_filenames(pristine, |candidate| {
            asset_manager.get_ref(candidate).is_some()
        });
        if !siblings.is_empty() {
            groups += 1;
            sibling_files += siblings.len();
            max_total_files = max_total_files.max(siblings.len() + 1);
        }
    }
    log::info!(
        "Theater TMP variants: {} contiguous sibling files across {} groups (max total files {})",
        sibling_files,
        groups,
        max_total_files,
    );
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn resolve_contiguous_variant_chains_for_test(
    lookup: &mut TilesetLookup,
    asset_manager: &AssetManager,
) {
    resolve_contiguous_variant_chains(lookup, asset_manager);
}

fn resolve_tileset_start(lookup: &TilesetLookup, ordinal: i32) -> Option<u16> {
    if ordinal < 0 {
        return None;
    }
    lookup
        .bounds()
        .get(ordinal as usize)
        .map(|bounds| bounds.start)
}

fn resolve_rmg_tile_keys(
    lookup: &TilesetLookup,
    general: Option<&IniSection>,
    slope_set_pieces: Option<u16>,
) -> RmgTileKeys {
    let resolve = |key: &str| {
        let ordinal = general.map_or(-1, |section| section.read_int(key, -1));
        resolve_tileset_start(lookup, ordinal)
    };
    RmgTileKeys {
        clear_tile: resolve("ClearTile"),
        ramp_base: resolve("RampBase"),
        ramp_smooth: resolve("RampSmooth"),
        // Reuse the existing General reader's ordinal. Original545E1D/545E26
        // publishes the cumulative base, not that ordinal, to ABC1F8.
        slope_set_pieces: resolve_tileset_start(lookup, slope_set_pieces.map_or(-1, i32::from)),
        rough_tile: resolve("RoughTile"),
        sand_tile: resolve("SandTile"),
        green_tile: resolve("GreenTile"),
        clear_to_rough_lat: resolve("ClearToRoughLat"),
        clear_to_sand_lat: resolve("ClearToSandLat"),
        clear_to_green_lat: resolve("ClearToGreenLat"),
        clear_to_pave_lat: resolve("ClearToPaveLat"),
        pave_tile: resolve("PaveTile"),
        water_set: resolve("WaterSet"),
        shore_pieces: resolve("ShorePieces"),
        water_bridge: resolve("WaterBridge"),
        misc_pave_tile: resolve("MiscPaveTile"),
        paved_roads: resolve("PavedRoads"),
        paved_road_ends: resolve("PavedRoadEnds"),
        medians: resolve("Medians"),
    }
}

fn resolve_cliff_ranges(
    lookup: &TilesetLookup,
    general: Option<&IniSection>,
    bridge_set: Option<u16>,
    wood_bridge_set: Option<u16>,
) -> TheaterCliffRanges {
    let resolve = |key: &str, default: i32| {
        let ordinal = general.map_or(default, |section| section.read_int(key, default));
        resolve_tileset_start(lookup, ordinal)
    };
    TheaterCliffRanges {
        cliff_set: resolve("CliffSet", -1),
        cliff_ramps: resolve("CliffRamps", -1),
        water_cliffs: resolve("WaterCliffs", -1),
        // ReadTheater545150's call545978 uses -2, unlike the other ordinals.
        destroyable_cliffs: resolve("DestroyableCliffs", -2),
        bridge_set: resolve_tileset_start(lookup, bridge_set.map_or(-1, i32::from)),
        wood_bridge_set: resolve_tileset_start(lookup, wood_bridge_set.map_or(-1, i32::from)),
        water_caves: resolve("WaterCaves", -1),
        waterfall_east: resolve("WaterfallEast", -1),
        waterfall_west: resolve("WaterfallWest", -1),
        waterfall_north: resolve("WaterfallNorth", -1),
        waterfall_south: resolve("WaterfallSouth", -1),
    }
}

fn apply_lunar_global_zeroing(
    theater_name: &str,
    bridge_set: &mut Option<u16>,
    wood_bridge_set: &mut Option<u16>,
    cliff_ranges: &mut TheaterCliffRanges,
    rmg_tiles: &mut RmgTileKeys,
) {
    if !theater_name.eq_ignore_ascii_case("LUNAR") {
        return;
    }
    *bridge_set = None;
    *wood_bridge_set = None;
    // Original ReadTheater545150, 546C95..546CB9 (original executable;
    // General reader identities: tools/rules_oracle/theater_general_reader.md).
    // Lunar resets these seven globals, retaining the other declared ranges.
    cliff_ranges.cliff_set = None;
    cliff_ranges.water_cliffs = None;
    cliff_ranges.bridge_set = None;
    cliff_ranges.wood_bridge_set = None;
    rmg_tiles.water_set = None;
    rmg_tiles.shore_pieces = None;
    rmg_tiles.water_bridge = None;
}

fn waterfall_is_special(start: Option<u16>, tile_id: u16, sub_tile: u8, ordinary: &[u8]) -> bool {
    waterfall_blocks(start, tile_id, sub_tile, ordinary)
}

/// Checked tile-index projection; absent, negative and out-of-range values
/// have no representable u16 tile identity. Native signed repair keys remain
/// owned by HighBridgeRimTiles and must not be reconstructed from this view.
fn read_general_u16(general: Option<&IniSection>, key: &str) -> Option<u16> {
    u16::try_from(general?.read_int(key, -1)).ok()
}

fn load_exact_palette(
    asset_manager: &AssetManager,
    name: &str,
    theater_name: &str,
    palette_kind: &str,
) -> Option<Palette> {
    if let Some((data, source)) = asset_manager.get_with_source(name)
        && let Ok(palette) = Palette::from_bytes_gamemd_ui(&data)
    {
        log::info!(
            "Theater {}: {} palette '{}' from {}",
            theater_name,
            palette_kind,
            name,
            source
        );
        return Some(palette);
    }
    log::warn!(
        "Theater {}: {} palette '{}' unavailable",
        theater_name,
        palette_kind,
        name
    );
    None
}

fn native_missing_theater_palette() -> Palette {
    let mut colors = [Color::rgb(0, 0, 0); 256];
    for (index, color) in colors.iter_mut().enumerate() {
        let value = index as u8;
        *color = Color {
            r: value,
            g: 255u8.wrapping_sub(value),
            b: value.wrapping_shl(2),
            a: if index == 0 { 0 } else { 255 },
        };
    }
    Palette { colors }
}

/// Collect all unique TileKey values used by a terrain grid.
/// Filters out NO_TILE (-1) and negative indices, truncates to u16 for TileKey.
pub fn collect_used_tiles(cells: &[(i32, u8)]) -> HashSet<TileKey> {
    cells
        .iter()
        .filter(|(id, _)| *id >= 0)
        .map(|&(tile_index, sub_tile)| TileKey {
            variant: 0,
            tile_id: tile_index as u16,
            sub_tile,
        })
        .collect()
}

/// Resolve the active positive subtile through the pristine/variant template
/// grid while leaving the map-requested u8 identity available to atlas keys.
pub(crate) fn wrapped_subtile_index(
    sub_tile: u8,
    template_width: u32,
    template_height: u32,
) -> Option<usize> {
    let cell_count = template_width.checked_mul(template_height)?;
    if cell_count == 0 {
        return None;
    }
    usize::try_from(u32::from(sub_tile) % cell_count).ok()
}

/// Inject TileKey entries for the 8 bridge anchor variant tile_ids × all
/// sub_tiles in each tile_id's TMP template into the `needed` set used by
/// the atlas pre-loader.
///
/// Required so the atlas has the variant tiles loaded before any bridge
/// damage happens at runtime — without this, the first damage hit would be
/// an atlas miss for the variant cell, producing a blank or fallback sprite
/// on the same tick that the damage applies.
///
/// Silently skips tile_ids whose TMP file is absent from `asset_manager`
/// (e.g., mod theaters missing a variant TMP). Logs one `WARN` per missing
/// TMP at theater load.
pub fn inject_bridge_middle_tiles(
    needed: &mut HashSet<TileKey>,
    table: &BridgeMiddleTiles,
    lookup: &TilesetLookup,
    asset_manager: &crate::assets::asset_manager::AssetManager,
) {
    for &tile_id in &table.tile_ids {
        let Some(filename) = lookup.filename(tile_id as i32) else {
            log::warn!(
                "Bridge anchor variant tile_id {} has no entry in TilesetLookup; skipping pre-load",
                tile_id
            );
            continue;
        };
        let filename: String = filename.to_string();
        let Some(tmp_data) = asset_manager.get_ref(&filename) else {
            log::warn!(
                "Bridge anchor variant TMP {} missing from MIX archives; cell will render as native tile_id on damage",
                filename
            );
            continue;
        };
        let tmp = match TmpFile::from_bytes(tmp_data) {
            Ok(t) => t,
            Err(e) => {
                log::warn!(
                    "Bridge anchor variant TMP {} failed to parse: {:#}",
                    filename,
                    e
                );
                continue;
            }
        };
        let cell_count = (tmp.template_width * tmp.template_height) as usize;
        for sub_tile in 0..cell_count {
            if tmp.tiles[sub_tile].is_none() {
                continue;
            }
            needed.insert(TileKey {
                tile_id,
                sub_tile: sub_tile as u8,
                variant: 0,
            });
        }
    }
}

/// Load RGBA tile images for tiles needed by the map. Groups by tile_id
/// to batch-load TMP files. Skips missing or unparseable TMP files.
pub fn load_tile_images(
    asset_manager: &AssetManager,
    lookup: &TilesetLookup,
    palette: &Palette,
    needed: &HashSet<TileKey>,
) -> HashMap<TileKey, TileImage> {
    let mut images: HashMap<TileKey, TileImage> = HashMap::new();
    let mut blank_slot_count: u32 = 0;
    let mut out_of_range_count: u32 = 0;
    let mut missing_file_count: u32 = 0;
    let mut empty_cell_count: u32 = 0;
    let mut parse_error_count: u32 = 0;
    // Diagnostic samples: up to 16 distinct tile_ids per failure bucket.
    // Lets the load log show which specific tile ranges are unresolved without
    // flooding output. Each tile_id appears at most once because the outer loop
    // is keyed on a distinct tile_id.
    let mut out_of_range_samples: Vec<u16> = Vec::new();
    let mut blank_slot_samples: Vec<u16> = Vec::new();
    let mut missing_file_samples: Vec<(u16, String)> = Vec::new();
    const SAMPLE_CAP: usize = 16;

    // Group needed tiles by tile_id to batch-load TMP files.
    let mut by_tile_id: HashMap<u16, Vec<u8>> = HashMap::new();
    for key in needed {
        by_tile_id
            .entry(key.tile_id)
            .or_default()
            .push(key.sub_tile);
    }

    for (tile_id, sub_tiles) in &by_tile_id {
        let tile_id_i32: i32 = *tile_id as i32;

        // Distinguish out-of-range from blank slots.
        if tile_id_i32 >= lookup.len() as i32 {
            out_of_range_count += sub_tiles.len() as u32;
            if out_of_range_samples.len() < SAMPLE_CAP {
                out_of_range_samples.push(*tile_id);
            }
            continue;
        }

        let filename: &str = match lookup.filename(tile_id_i32) {
            Some(f) => f,
            None => {
                blank_slot_count += sub_tiles.len() as u32;
                if blank_slot_samples.len() < SAMPLE_CAP {
                    blank_slot_samples.push(*tile_id);
                }
                continue;
            }
        };

        let tmp_data: &[u8] = match asset_manager.get_ref(filename) {
            Some(d) => d,
            None => {
                missing_file_count += sub_tiles.len() as u32;
                if missing_file_samples.len() < SAMPLE_CAP {
                    missing_file_samples.push((*tile_id, filename.to_string()));
                }
                log::trace!("TMP not found: {} (tile_id {})", filename, tile_id);
                continue;
            }
        };

        let tmp: TmpFile = match TmpFile::from_bytes(tmp_data) {
            Ok(t) => t,
            Err(e) => {
                parse_error_count += sub_tiles.len() as u32;
                log::warn!("TMP parse error {}: {:#}", filename, e);
                continue;
            }
        };

        // Log tile dimensions from first successfully parsed TMP file.
        if images.is_empty() {
            log::info!(
                "First TMP '{}': tile_width={}, tile_height={}, template={}x{}",
                filename,
                tmp.tile_width,
                tmp.tile_height,
                tmp.template_width,
                tmp.template_height,
            );
        }

        for &sub in sub_tiles {
            let Some(source_sub) =
                wrapped_subtile_index(sub, tmp.template_width, tmp.template_height)
            else {
                empty_cell_count += 1;
                continue;
            };

            let Some(tile) = tmp.tiles.get(source_sub).and_then(|tile| tile.as_ref()) else {
                empty_cell_count += 1;
                continue;
            };
            match tmp.tile_to_rgba(source_sub, palette) {
                Ok(rgba) => {
                    images.insert(
                        TileKey {
                            tile_id: *tile_id,
                            sub_tile: sub,
                            variant: 0,
                        },
                        TileImage {
                            rgba,
                            depth: tile.depth.clone(),
                            width: tile.pixel_width,
                            height: tile.pixel_height,
                            offset_x: tile.offset_x,
                            offset_y: tile.offset_y,
                        },
                    );
                }
                Err(e) => {
                    log::warn!("RGBA convert error {} sub {}: {:#}", filename, sub, e);
                }
            }
        }
    }

    // Load the exact contiguous sibling chains resolved after theater archive
    // activation. Each independent TMP gets its own positive variant index.
    let mut variant_count: u32 = 0;
    for (tile_id, sub_tiles) in &by_tile_id {
        let var_names = lookup.variant_filenames(*tile_id);
        if var_names.is_empty() {
            continue;
        }
        for (var_idx, var_name) in var_names.iter().enumerate() {
            let Some(var_data) = asset_manager.get_ref(var_name) else {
                log::warn!("Resolved TMP variant disappeared during atlas load: {var_name}");
                break;
            };
            let Ok(var_tmp) = TmpFile::from_bytes(var_data) else {
                break;
            };
            for &sub in sub_tiles {
                let Some(source_sub) =
                    wrapped_subtile_index(sub, var_tmp.template_width, var_tmp.template_height)
                else {
                    continue;
                };
                let Some(tile) = var_tmp.tiles.get(source_sub).and_then(|tile| tile.as_ref())
                else {
                    continue;
                };
                if let Ok(rgba) = var_tmp.tile_to_rgba(source_sub, palette) {
                    images.insert(
                        TileKey {
                            tile_id: *tile_id,
                            sub_tile: sub,
                            variant: (var_idx + 1) as u8,
                        },
                        TileImage {
                            rgba,
                            depth: tile.depth.clone(),
                            width: tile.pixel_width,
                            height: tile.pixel_height,
                            offset_x: tile.offset_x,
                            offset_y: tile.offset_y,
                        },
                    );
                    variant_count += 1;
                }
            }
        }
    }

    log::info!(
        "Tile loading: {} loaded ({} variants), {} empty cells (expected), {} blank slots, \
         {} missing files, {} out-of-range, {} parse errors (of {} needed)",
        images.len(),
        variant_count,
        empty_cell_count,
        blank_slot_count,
        missing_file_count,
        out_of_range_count,
        parse_error_count,
        needed.len()
    );
    if !out_of_range_samples.is_empty() {
        log::info!(
            "  out-of-range tile_id samples (max {}): {:?} (lookup.len={})",
            SAMPLE_CAP,
            out_of_range_samples,
            lookup.len()
        );
    }
    if !blank_slot_samples.is_empty() {
        log::info!(
            "  blank-slot tile_id samples (max {}): {:?}",
            SAMPLE_CAP,
            blank_slot_samples
        );
    }
    if !missing_file_samples.is_empty() {
        log::info!(
            "  missing-file samples (max {}): {:?}",
            SAMPLE_CAP,
            missing_file_samples
        );
    }

    images
}

// Tests extracted to map/theater_tests.rs to stay under 400 lines.
#[cfg(test)]
#[path = "theater_tests.rs"]
mod theater_tests;
