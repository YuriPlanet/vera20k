//! Parser for RA2 .map files (INI format with binary IsoMapPack5 terrain data).
//!
//! RA2 maps are INI files with special sections:
//! - `[Map]`: metadata (theater, size, local bounds)
//! - `[IsoMapPack5]`: base64-encoded, LZO-compressed terrain cell data
//!
//! Each terrain cell is 11 bytes: x(i16) y(u16) tile_index(i32)
//! sub_tile(u8) level(u8) ice_growth(u8). Cells describe the isometric tile grid.
//! (Confirmed by ModEnc IsoMapPack5 docs + FinalAlert2 EA source release.)
//!
//! ## Dependency rules
//! - Part of map/ — depends on assets/ (MIX archives), rules/ (INI parser), util/ (base64, lzo).

use std::collections::HashMap;
#[cfg(any(test, feature = "test-support"))]
use std::path::Path;

use crate::assets::error::AssetError;
use crate::assets::mix_archive::MixArchive;
use crate::map::actions::{self, ActionMap};
use crate::map::basic::{self, BasicSection, SpecialFlagsSection};
use crate::map::entities::{self, MapEntity};
use crate::map::events::{self, EventMap};
use crate::map::overlay::{
    self, OverlayDataPack, OverlayEntry, OverlayIdentityPack, TerrainObject,
};
use crate::map::preview::{self, PreviewSection};
use crate::map::tags::{self, TagMap};
use crate::map::triggers::{self, TriggerMap};
use crate::map::variable_names::{self, LocalVariableMap};
use crate::map::waypoints::{self, Waypoint};
use crate::rules::error::RulesError;
use crate::rules::ini_parser::IniFile;
use crate::rules::ini_value::{crt_atoi, strtok};
use crate::util::base64;
use crate::util::lzo::{self, LzoError};

/// Size of one terrain cell record in the decompressed IsoMapPack5 data.
const CELL_RECORD_SIZE: usize = 11;
/// Size of the coordinate header that precedes each terrain cell payload.
const CELL_HEADER_SIZE: usize = 4;
/// Native IsoMapPack5 lookup dimensions.
const ISO_MAP_ROW_WIDTH: i32 = 512;
const ISO_MAP_CELL_COUNT: i32 = ISO_MAP_ROW_WIDTH * ISO_MAP_ROW_WIDTH;
const DEFAULT_SIZE_RECT: [i32; 4] = [1, 1, 50, 50];

/// Errors during map file parsing.
#[derive(Debug)]
pub enum MapError {
    Ini(RulesError),
    MissingSection { name: String },
    MissingIsoMapPack,
    Base64(String),
    Lzo(LzoError),
    Asset(AssetError),
    CellDataTruncated { expected: usize, actual: usize },
    TilesetRegistryTooLarge { attempted: usize, maximum: usize },
    TilesetOrdinalOverflow { ordinal: u32, maximum: u32 },
    Io(std::io::Error),
}

impl std::fmt::Display for MapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MapError::Ini(e) => write!(f, "Map INI error: {}", e),
            MapError::MissingSection { name } => write!(f, "Missing [{}] section in map", name),
            MapError::MissingIsoMapPack => write!(f, "No [IsoMapPack5] data in map"),
            MapError::Base64(e) => write!(f, "Base64 decode error: {}", e),
            MapError::Lzo(e) => write!(f, "LZO decompress error: {}", e),
            MapError::Asset(e) => write!(f, "Asset error: {}", e),
            MapError::CellDataTruncated { expected, actual } => {
                write!(
                    f,
                    "Cell data truncated: need {} bytes, got {}",
                    expected, actual
                )
            }
            MapError::TilesetRegistryTooLarge { attempted, maximum } => write!(
                f,
                "Theater tileset registry requires {attempted} slots, but Rust can represent at most {maximum} usable tile IDs"
            ),
            MapError::TilesetOrdinalOverflow { ordinal, maximum } => write!(
                f,
                "Theater tileset ordinal {ordinal} exceeds the supported maximum {maximum}"
            ),
            MapError::Io(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for MapError {}

impl From<RulesError> for MapError {
    fn from(e: RulesError) -> Self {
        MapError::Ini(e)
    }
}
impl From<LzoError> for MapError {
    fn from(e: LzoError) -> Self {
        MapError::Lzo(e)
    }
}
impl From<AssetError> for MapError {
    fn from(e: AssetError) -> Self {
        MapError::Asset(e)
    }
}
impl From<std::io::Error> for MapError {
    fn from(e: std::io::Error) -> Self {
        MapError::Io(e)
    }
}

/// Map header extracted from the [Map] INI section.
#[derive(Debug, Clone)]
pub struct MapHeader {
    /// Theater name: "TEMPERATE", "SNOW", "URBAN", etc.
    pub theater: String,
    /// Raw `[Map] Fill` value. Active YR defaults this to `Clear`; terrain
    /// initialization interprets the verified `Water` spelling.
    pub fill: String,
    /// Signed `[Map] Level` value. Map allocation consumes its low byte before
    /// explicit IsoMapPack records replace individual cell levels.
    pub level: i32,
    /// Full map width (from Size= 3rd value).
    pub width: u32,
    /// Full map height (from Size= 4th value).
    pub height: u32,
    /// Playable area left (from LocalSize= 1st value).
    pub local_left: u32,
    /// Playable area top (from LocalSize= 2nd value).
    pub local_top: u32,
    /// Playable area width (from LocalSize= 3rd value).
    pub local_width: u32,
    /// Playable area height (from LocalSize= 4th value).
    pub local_height: u32,
}

/// A pre-placed smudge entry from the map's `[Smudge]` section.
///
/// Parsed format: `Key=TYPENAME,X,Y,IsBaked`.
/// Entries with `IsBaked != 0` are SKIPPED at parse time (they represent
/// smudges already baked into the underlying tile graphic).
#[derive(Debug, Clone)]
pub struct MapSmudgeEntry {
    pub type_name: String,
    pub rx: u16,
    pub ry: u16,
}

/// A single isometric terrain cell from IsoMapPack5.
///
/// Layout per ModEnc + FinalAlert2 source: 11 bytes total.
/// `tile_index` is the signed raw value from the pack. Map materialization
/// translates it through the active theater's `LastTilesInSet` table before
/// treating it as an actual cumulative tileset index. Consequently, raw `-1`
/// is not intrinsically the runtime no-tile sentinel on a legacy/custom
/// theater with a reached compatibility record.
#[derive(Debug, Clone)]
pub struct MapCell {
    /// Canonical isometric X coordinate after the 512-wide native lookup.
    pub rx: u16,
    /// Canonical isometric Y coordinate after the 512-wide native lookup.
    pub ry: u16,
    /// Signed raw Pack5 tile value (i32, NOT u16). After the map-load
    /// compatibility seam, this field holds the translated actual tile index.
    pub tile_index: i32,
    /// Sub-tile index within a multi-cell TMP template (0 for single-cell tiles).
    pub sub_tile: u8,
    /// Elevation level (0 = ground, higher = elevated). Each level ~15px visual shift.
    pub z: u8,
}

/// One native fixed-table lookup performed while decoding IsoMapPack5.
///
/// Payload-bearing `MapCell`s retain canonical coordinates; this separate,
/// load-only trace preserves the raw request needed when the lookup resolves
/// to MapClass's shared dummy instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IsoMapPackLookup {
    pub(crate) raw_x: i16,
    pub(crate) raw_y: u16,
    pub(crate) canonical: Option<(u16, u16)>,
}

#[derive(Debug)]
struct ParsedIsoMapPack {
    cells: Vec<MapCell>,
    lookups: Vec<IsoMapPackLookup>,
}

/// The only capability accepted by the authored pack finalizer. Production
/// code can obtain it only by moving the one slot out of `MapFile`.
#[derive(Debug)]
pub(crate) struct AuthoredOverlayPackReceipt {
    identity: OverlayIdentityPack,
    data: OverlayDataPack,
}

impl AuthoredOverlayPackReceipt {
    pub(crate) fn into_parts(self) -> (OverlayIdentityPack, OverlayDataPack) {
        (self.identity, self.data)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn from_parts_for_test(
        identity: OverlayIdentityPack,
        data: OverlayDataPack,
    ) -> Self {
        Self { identity, data }
    }
}

/// Non-Clone single owner for the two independently gated raw pack bodies.
/// The public(crate) empty constructor exists only so synthetic `MapFile`
/// literals can represent a map with no bodies; it cannot mint or duplicate a
/// parsed receipt.
#[derive(Debug)]
pub struct AuthoredOverlayPackSlot {
    receipt: Option<AuthoredOverlayPackReceipt>,
}

impl AuthoredOverlayPackSlot {
    pub fn empty() -> Self {
        Self {
            receipt: Some(AuthoredOverlayPackReceipt {
                identity: OverlayIdentityPack::default(),
                data: OverlayDataPack::default(),
            }),
        }
    }

    fn from_parts(identity: OverlayIdentityPack, data: OverlayDataPack) -> Self {
        Self {
            receipt: Some(AuthoredOverlayPackReceipt { identity, data }),
        }
    }

    fn receipt(&self) -> Result<&AuthoredOverlayPackReceipt, OverlayPacksAlreadyConsumed> {
        self.receipt.as_ref().ok_or(OverlayPacksAlreadyConsumed)
    }

    fn receipt_mut(
        &mut self,
    ) -> Result<&mut AuthoredOverlayPackReceipt, OverlayPacksAlreadyConsumed> {
        self.receipt.as_mut().ok_or(OverlayPacksAlreadyConsumed)
    }

    fn take(&mut self) -> Result<AuthoredOverlayPackReceipt, OverlayPacksAlreadyConsumed> {
        self.receipt.take().ok_or(OverlayPacksAlreadyConsumed)
    }
}

/// A parsed RA2 map file.
#[derive(Debug)]
pub struct MapFile {
    pub header: MapHeader,
    /// Parsed `[Basic]` metadata such as title and briefing hooks.
    pub basic: BasicSection,
    /// Parsed campaign score text and difficulty par times from `[Ranking]`.
    pub ranking: crate::map::ranking::RankingSection,
    /// Parsed preview metadata from `[Preview]` / `[PreviewPack]`.
    pub preview: PreviewSection,
    pub cells: Vec<MapCell>,
    /// Ordered native lookup evidence for production IsoMapPack materialization.
    /// Synthetic and RMG maps leave this empty and use their explicit cells.
    pub iso_map_pack_lookups: Vec<IsoMapPackLookup>,
    /// Entity placements from [Units], [Infantry], [Structures], [Aircraft] sections.
    pub entities: Vec<MapEntity>,
    /// Overlay objects from [OverlayPack] + [OverlayDataPack] (ore, walls, fences, etc.).
    pub overlays: Vec<OverlayEntry>,
    /// Single raw `[OverlayPack]` / `[OverlayDataPack]` authority. Deliberately
    /// non-Clone and moved exactly once into the authored finalizer.
    pub authored_overlay_packs: AuthoredOverlayPackSlot,
    /// Pre-placed smudges from the map's `[Smudge]` section.
    /// `IsBaked != 0` entries are filtered at parse time.
    pub smudges: Vec<MapSmudgeEntry>,
    /// Terrain objects from [Terrain] section (trees, cacti, rocks).
    pub terrain_objects: Vec<TerrainObject>,
    /// Waypoint index -> cell coordinate mapping from [Waypoints].
    pub waypoints: HashMap<u32, Waypoint>,
    /// Exact key -> tag record from [Tags]. `[CellTags]` has one reader:
    /// the trigger runtime's native `Read_Map_Section` port.
    pub tags: TagMap,
    /// Exact key -> trigger header from [Triggers].
    pub triggers: TriggerMap,
    /// Exact key -> event record from [Events].
    pub events: EventMap,
    /// Exact key -> action record from [Actions].
    pub actions: ActionMap,
    /// Local variable definitions from [VariableNames].
    pub local_variables: LocalVariableMap,
    /// Parsed `[SpecialFlags]` section (TiberiumGrows, TiberiumSpreads overrides).
    pub special_flags: SpecialFlagsSection,
    /// Full parsed INI for accessing additional sections (e.g., [Houses]).
    pub ini: IniFile,
}

impl MapFile {
    /// Parse a map from raw INI bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, MapError> {
        Self::from_ini(IniFile::from_bytes(data)?)
    }

    fn from_ini(ini: IniFile) -> Result<Self, MapError> {
        let header: MapHeader = parse_header(&ini)?;
        let basic: BasicSection = basic::parse_basic_section(&ini);
        let ranking: crate::map::ranking::RankingSection =
            crate::map::ranking::parse_ranking_section(&ini);
        let special_flags: SpecialFlagsSection = basic::parse_special_flags_section(&ini);
        let mut preview: PreviewSection = preview::parse_preview_section(&ini);
        match preview::decode_preview_image_from_ini(&ini) {
            Ok(Some(decoded)) => preview.decoded = Some(decoded),
            Ok(None) => {
                log::warn!(
                    "Map preview unavailable; continuing without preview because [Preview] size or [PreviewPack] data is missing"
                );
            }
            Err(err) => {
                log::warn!("Map preview decode failed; continuing without preview: {err}");
            }
        }
        let iso_map_pack = parse_iso_map_pack(&ini)?;
        let entities: Vec<MapEntity> = entities::parse_map_entities(&ini);
        let overlay::ParsedOverlayPacks {
            identity: overlay_identity,
            entries: overlays,
            data: overlay_data,
        } = overlay::parse_overlay_packs(&ini);
        let terrain_objects: Vec<TerrainObject> = overlay::parse_terrain_objects(&ini);
        let smudges: Vec<MapSmudgeEntry> = parse_map_smudges(&ini);
        let waypoints: HashMap<u32, Waypoint> = waypoints::parse_waypoints(&ini);
        let tags: TagMap = tags::parse_tags(&ini);
        let triggers: TriggerMap = triggers::parse_triggers(&ini);
        let events: EventMap = events::parse_events(&ini);
        let actions: ActionMap = actions::parse_actions(&ini);
        let local_variables: LocalVariableMap = variable_names::parse_local_variables(&ini);
        Ok(MapFile {
            header,
            basic,
            ranking,
            preview,
            cells: iso_map_pack.cells,
            iso_map_pack_lookups: iso_map_pack.lookups,
            entities,
            overlays,
            authored_overlay_packs: AuthoredOverlayPackSlot::from_parts(
                overlay_identity,
                overlay_data,
            ),
            smudges,
            terrain_objects,
            waypoints,
            tags,
            triggers,
            events,
            actions,
            local_variables,
            special_flags,
            ini,
        })
    }

    pub fn overlay_data_pack(&self) -> Result<&OverlayDataPack, OverlayPacksAlreadyConsumed> {
        self.authored_overlay_packs
            .receipt()
            .map(|packs| &packs.data)
    }

    pub(crate) fn replace_unconsumed_overlay_data(
        &mut self,
        data: OverlayDataPack,
    ) -> Result<(), OverlayPacksAlreadyConsumed> {
        self.authored_overlay_packs.receipt_mut()?.data = data;
        Ok(())
    }

    pub(crate) fn take_authored_overlay_packs(
        &mut self,
    ) -> Result<AuthoredOverlayPackReceipt, OverlayPacksAlreadyConsumed> {
        self.authored_overlay_packs.take()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("authored OverlayPack/OverlayDataPack receipt was already consumed")]
pub struct OverlayPacksAlreadyConsumed;

/// Load a map file from disk, auto-detecting MIX-wrapped vs raw INI.
///
/// Retail map files (`.mmx`, `.yro`, sometimes `.map`) are MIX archives
/// containing two entries: the actual map INI and a tiny `[MultiMaps]`
/// description stub. Editor-saved maps (`.map`, `.mpr`, `.yrm`) are raw INI
/// text. We dispatch on the first two header bytes — `00 00` is the
/// new-format MIX marker; anything else is treated as INI text.
#[cfg(any(test, feature = "test-support"))]
pub fn load_from_path(path: &Path) -> Result<MapFile, MapError> {
    load_from_bytes(std::fs::read(path)?)
}

/// Parse one consumed file buffer, auto-detecting MIX-wrapped vs raw INI.
///
/// Callers recording source identity can hash this same buffer before passing
/// ownership here. Wrapped maps retain the existing inner-entry selection.
pub fn load_from_bytes(bytes: Vec<u8>) -> Result<MapFile, MapError> {
    MapFile::from_ini(ini_from_file_bytes(bytes)?)
}

/// Select and parse the map INI without decoding terrain or preview packs.
/// Menu metadata and full loading share this raw/wrapped-file decision.
pub(crate) fn ini_from_file_bytes(bytes: Vec<u8>) -> Result<IniFile, MapError> {
    if is_mix_header(&bytes) {
        let archive: MixArchive = MixArchive::from_bytes(bytes)?;
        pick_map_ini(&archive)
    } else {
        Ok(IniFile::from_bytes(&bytes)?)
    }
}

/// New-format MIX marker: first two bytes are `0x00 0x00`.
fn is_mix_header(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes[0] == 0 && bytes[1] == 0
}

/// Pick the inner MIX entry that contains the actual map INI.
///
/// Retail map MIXes always pack two entries: the real map (~100 KB+) and a
/// ~120-byte `[MultiMaps]` description stub. We try entries in descending
/// size order and return the first one whose bytes parse as INI containing
/// a `[Map]` section.
fn pick_map_ini(archive: &MixArchive) -> Result<IniFile, MapError> {
    let mut entries: Vec<_> = archive.entries().to_vec();
    if entries.is_empty() {
        return Err(MapError::MissingIsoMapPack);
    }
    entries.sort_by_key(|a| std::cmp::Reverse(a.size));
    for entry in &entries {
        let Some(data) = archive.get_by_id(entry.id) else {
            continue;
        };
        if let Ok(ini) = IniFile::from_bytes(data)
            && ini.section("Map").is_some()
        {
            return Ok(ini);
        }
    }
    Err(MapError::MissingIsoMapPack)
}

/// Extract the [Map] header fields.
fn parse_header(ini: &IniFile) -> Result<MapHeader, MapError> {
    let map_section = ini
        .section("Map")
        .ok_or(MapError::MissingSection { name: "Map".into() })?;

    // `Read_Map_Section_And_IsoMapPacks @ 0x004ACE70`: the theater name read
    // (`0x00475870`, 0x80 bytes), `Fill` as a 0x20-byte ReadString, ReadInt
    // `Level`, and ReadRect for both rectangles.
    let theater: String = map_section
        .read_name("Theater", 0x80)
        .unwrap_or("TEMPERATE")
        .to_uppercase();
    let fill = map_section.read_string("Fill", "Clear", 0x20);
    let level = map_section.read_int("Level", 0);

    // Full-map resizing stores Size width/height but normalizes its origin before LocalSize is read.
    let size_parts = map_section.read_rect("Size", DEFAULT_SIZE_RECT);
    let normalized_size = [0, 0, size_parts[2], size_parts[3]];
    let local_parts = map_section.read_rect("LocalSize", normalized_size);

    Ok(MapHeader {
        theater,
        fill,
        level,
        width: size_parts[2] as u32,
        height: size_parts[3] as u32,
        local_left: local_parts[0] as u32,
        local_top: local_parts[1] as u32,
        local_width: local_parts[2] as u32,
        local_height: local_parts[3] as u32,
    })
}

/// Extract and decode the [IsoMapPack5] terrain data.
///
/// 1. Concatenate all numbered key values from the section.
/// 2. Base64 decode the concatenated string.
/// 3. LZO decompress the chunks.
/// 4. Parse 11-byte terrain cell records.
fn parse_iso_map_pack(ini: &IniFile) -> Result<ParsedIsoMapPack, MapError> {
    let section = ini
        .section("IsoMapPack5")
        .ok_or(MapError::MissingIsoMapPack)?;

    let b64_data = section.read_packed_text();

    if b64_data.is_empty() {
        return Err(MapError::MissingIsoMapPack);
    }

    // Base64 decode → LZO decompress.
    let compressed: Vec<u8> = base64::base64_decode(&b64_data).map_err(MapError::Base64)?;
    let decompressed: Vec<u8> = lzo::decompress_chunks(&compressed)?;

    let parsed = parse_iso_map_pack_records(&decompressed)?;

    // Diagnostic: tile_index distribution. Lets a reader of the load logs see
    // how high a map's IsoMapPack5 reaches vs. what the theater INI defines.
    let mut min_idx: i32 = i32::MAX;
    let mut max_idx: i32 = i32::MIN;
    let mut raw_negative: usize = 0;
    let mut no_tile_sentinel: usize = 0;
    let mut distinct: std::collections::HashSet<i32> = std::collections::HashSet::new();
    for c in &parsed.cells {
        if c.tile_index < 0 {
            raw_negative += 1;
        }
        if c.tile_index == i32::from(u16::MAX) {
            no_tile_sentinel += 1;
        }
        min_idx = min_idx.min(c.tile_index);
        max_idx = max_idx.max(c.tile_index);
        distinct.insert(c.tile_index);
    }
    log::info!(
        "IsoMapPack5: {} cells, {} raw-negative, {} positive-0xFFFF no-tile, tile_index min={}, max={}, distinct={}",
        parsed.cells.len(),
        raw_negative,
        no_tile_sentinel,
        if min_idx == i32::MAX { -1 } else { min_idx },
        if max_idx == i32::MIN { -1 } else { max_idx },
        distinct.len()
    );

    Ok(parsed)
}

/// Parse native IsoMapPack5 records after chunk decompression.
fn parse_iso_map_pack_records(decompressed: &[u8]) -> Result<ParsedIsoMapPack, MapError> {
    let mut cells: Vec<MapCell> = Vec::with_capacity(decompressed.len() / CELL_RECORD_SIZE);
    let mut lookups: Vec<IsoMapPackLookup> =
        Vec::with_capacity(decompressed.len() / CELL_RECORD_SIZE);
    let mut offset: usize = 0;

    while offset < decompressed.len() {
        let remaining: usize = decompressed.len() - offset;
        if remaining < CELL_HEADER_SIZE {
            return Err(MapError::CellDataTruncated {
                expected: CELL_HEADER_SIZE,
                actual: remaining,
            });
        }

        let x: i16 = i16::from_le_bytes([decompressed[offset], decompressed[offset + 1]]);
        let y: u16 = u16::from_le_bytes([decompressed[offset + 2], decompressed[offset + 3]]);
        if x == 0 && y == 0 {
            break;
        }

        if remaining < CELL_RECORD_SIZE {
            return Err(MapError::CellDataTruncated {
                expected: CELL_RECORD_SIZE,
                actual: remaining,
            });
        }

        let d: &[u8] = &decompressed[offset..offset + CELL_RECORD_SIZE];
        let tile_index: i32 = i32::from_le_bytes([d[4], d[5], d[6], d[7]]);
        let sub_tile: u8 = d[8];
        let z: u8 = d[9];
        // d[10] is the legacy ice-growth byte; loading consumes but does not apply it here.
        offset += CELL_RECORD_SIZE;

        // gamemd.exe IsoMapPack5 decoder @ 0x0056BAC0: X is sign-extended,
        // Y is unsigned, and every non-sentinel header performs this fixed
        // 512-wide lookup before either applying or discarding its payload.
        let linear: i32 = i32::from(y) * ISO_MAP_ROW_WIDTH + i32::from(x);
        let canonical = (0..ISO_MAP_CELL_COUNT).contains(&linear).then_some((
            (linear % ISO_MAP_ROW_WIDTH) as u16,
            (linear / ISO_MAP_ROW_WIDTH) as u16,
        ));
        lookups.push(IsoMapPackLookup {
            raw_x: x,
            raw_y: y,
            canonical,
        });
        let Some((rx, ry)) = canonical else {
            continue;
        };

        cells.push(MapCell {
            rx,
            ry,
            tile_index,
            sub_tile,
            z,
        });
    }

    Ok(ParsedIsoMapPack { cells, lookups })
}

/// Parse pre-placed smudges from the map's `[Smudge]` section.
///
/// Section uses numbered keys: `0=TYPENAME,X,Y,IsBaked`. Entries with
/// `IsBaked != 0` are skipped (they're baked into the terrain graphic and
/// shouldn't be re-instantiated as runtime smudges).
fn parse_map_smudges(ini: &IniFile) -> Vec<MapSmudgeEntry> {
    let Some(section) = ini.section("Smudge") else {
        return Vec::new();
    };
    // The `[Smudge]` reader (`0x006B4C80`): a 0x80-byte ReadString split by
    // `strtok(",")`, coordinates as `(short)atoi`. Native reads a missing Y
    // as `atoi(NULL)`; Rust drops the row.
    let mut out: Vec<MapSmudgeEntry> = Vec::new();
    for key in section.keys() {
        let Some(value) = section.read_name(key, 0x80) else {
            continue;
        };
        let parts: Vec<&str> = strtok(value, &[',']).collect();
        if parts.len() < 3 {
            continue;
        }
        let is_baked = parts.get(3).map_or(0, |value| crt_atoi(value));
        if is_baked != 0 {
            continue;
        }
        let rx = crt_atoi(parts[1]) as i16 as u16;
        let ry = crt_atoi(parts[2]) as i16 as u16;
        out.push(MapSmudgeEntry {
            type_name: parts[0].to_uppercase(),
            rx,
            ry,
        });
    }
    out
}

#[cfg(test)]
mod smudge_parse_tests {
    use super::*;

    #[test]
    fn rejects_malformed_entries() {
        let ini = IniFile::from_bytes(
            b"[Smudge]\n\
              0=CR1,5,6\n\
              1=,5,6,0\n\
              2=CR1,X,6,0\n\
              3=CR1,5,6,0\n",
        )
        .unwrap();
        let smudges = parse_map_smudges(&ini);
        // Three-token entries default IsBaked to zero; malformed numerics use atoi's zero.
        // Entry 1: strtok skips the empty first field, so "5" is the type name.
        assert_eq!(smudges.len(), 4);
        assert_eq!(smudges[0].type_name, "CR1");
        assert_eq!((smudges[0].rx, smudges[0].ry), (5, 6));
        assert_eq!(smudges[1].type_name, "5");
        assert_eq!((smudges[1].rx, smudges[1].ry), (6, 0));
        assert_eq!((smudges[2].rx, smudges[2].ry), (0, 6));
        assert_eq!(smudges[3].type_name, "CR1");
    }

    #[test]
    fn gsi_04_11_smudge_parser_accepts_three_tokens_atoi_and_signed_short_narrowing() {
        let ini = IniFile::from_str(
            "[Smudge]\n\
             0=CR1,-1,5cells\n\
             1=CR2,40000,+7,0junk\n\
             2=BAKED,2,3,1tail\n",
        );
        let smudges = parse_map_smudges(&ini);
        assert_eq!(smudges.len(), 2);
        assert_eq!((smudges[0].rx, smudges[0].ry), (u16::MAX, 5));
        assert_eq!(smudges[1].rx, (40_000_i32 as i16) as u16);
        assert_eq!(smudges[1].ry, 7);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_CELL_ISO_MAP_PACK: &str = "DwALABwBAAIA/////wAAABEAAA==";
    const TWO_PIXEL_PREVIEW_PACK: &str = "CgAGABcBAgMEBQYRAAA=";

    fn full_map_with_preview(preview_sections: &str) -> Vec<u8> {
        format!(
            "[Map]\n\
             Theater=TEMPERATE\n\
             Size=0,0,2,1\n\
             LocalSize=0,0,2,1\n\
             [IsoMapPack5]\n\
             1={ONE_CELL_ISO_MAP_PACK}\n\
             {preview_sections}"
        )
        .into_bytes()
    }

    fn iso_map_record(
        x: i16,
        y: u16,
        tile_index: i32,
        sub_tile: u8,
        z: u8,
    ) -> [u8; CELL_RECORD_SIZE] {
        let mut record = [0u8; CELL_RECORD_SIZE];
        record[0..2].copy_from_slice(&x.to_le_bytes());
        record[2..4].copy_from_slice(&y.to_le_bytes());
        record[4..8].copy_from_slice(&tile_index.to_le_bytes());
        record[8] = sub_tile;
        record[9] = z;
        record
    }

    #[test]
    fn authored_overlay_pack_receipt_can_be_taken_only_once() {
        let mut map = MapFile::from_bytes(&full_map_with_preview("")).expect("minimal parsed map");
        let first = map
            .take_authored_overlay_packs()
            .expect("first authored receipt");
        let (identity, data) = first.into_parts();
        assert!(!identity.has_positive_length());
        assert!(!data.has_positive_length());
        assert!(matches!(
            map.take_authored_overlay_packs(),
            Err(OverlayPacksAlreadyConsumed)
        ));
        assert!(matches!(
            map.overlay_data_pack(),
            Err(OverlayPacksAlreadyConsumed)
        ));
    }

    #[test]
    fn gsi_04_02_map_fill_defaults_to_clear_and_preserves_present_value() {
        let default_ini = IniFile::from_str("[Map]\nSize=0,0,3,2\n");
        let default_header = parse_header(&default_ini).expect("default Fill parses");
        assert_eq!(default_header.fill, "Clear");

        let water_ini = IniFile::from_str("[Map]\nSize=0,0,3,2\nFill=  wAtEr \t\n");
        let water_header = parse_header(&water_ini).expect("present Fill parses");
        assert_eq!(water_header.fill, "wAtEr");
    }

    #[test]
    fn gsi_04_03a_map_level_defaults_and_preserves_signed_values() {
        let default_ini = IniFile::from_str("[Map]\nSize=0,0,3,2\n");
        let default_header = parse_header(&default_ini).expect("default Level parses");
        assert_eq!(default_header.level, 0);

        let present_ini = IniFile::from_str("[Map]\nSize=0,0,3,2\nLevel=260\n");
        let present_header = parse_header(&present_ini).expect("present Level parses");
        assert_eq!(present_header.level, 260);

        let negative_ini = IniFile::from_str("[Map]\nSize=0,0,3,2\nLevel=-2\n");
        let negative_header = parse_header(&negative_ini).expect("negative Level parses");
        assert_eq!(negative_header.level, -2);
    }

    #[test]
    fn gsi_04_01_loaded_empty_rect_values_are_omitted_and_become_missing_zeros() {
        let ini = IniFile::from_str("[Map]\nTheater=TEMPERATE\nSize=\nLocalSize= \t \n");
        let map_section = ini.section("Map").expect("Map section");

        assert!(map_section.get_for_test("Size").is_none());
        assert!(map_section.get_for_test("LocalSize").is_none());

        let header = parse_header(&ini).expect("omitted empty rectangles should remain loadable");
        assert_eq!(header.width, 0);
        assert_eq!(header.height, 0);
        assert_eq!(header.local_left, 0);
        assert_eq!(header.local_top, 0);
        assert_eq!(header.local_width, 0);
        assert_eq!(header.local_height, 0);
    }

    #[test]
    fn gsi_04_01_short_size_and_local_size_partially_overlay_defaults() {
        let ini = IniFile::from_str("[Map]\nSize=2,3,40\nLocalSize=5,6\n");

        let header = parse_header(&ini).expect("short rectangles retain unassigned defaults");

        assert_eq!(header.width, 40);
        assert_eq!(header.height, 50);
        assert_eq!(header.local_left, 5);
        assert_eq!(header.local_top, 6);
        assert_eq!(header.local_width, 40);
        assert_eq!(header.local_height, 50);
    }

    #[test]
    fn gsi_04_01_invalid_field_stops_after_prior_assignments() {
        let spaced = IniFile::from_str("[Map]\nSize=2 ,3,40,41\n");
        assert_eq!(
            spaced
                .section_or_empty("Map")
                .read_rect("Size", DEFAULT_SIZE_RECT),
            [2, 1, 50, 50]
        );

        let ini = IniFile::from_str("[Map]\nSize=2,invalid,40,41\nLocalSize=5,invalid,30,31\n");
        let header = parse_header(&ini).expect("valid prefixes should remain assigned");

        assert_eq!(header.width, 50);
        assert_eq!(header.height, 50);
        assert_eq!(header.local_left, 5);
        assert_eq!(header.local_top, 0);
        assert_eq!(header.local_width, 50);
        assert_eq!(header.local_height, 50);
    }

    #[test]
    fn gsi_04_01_signed_rect_fields_round_trip_through_u32_storage() {
        let ini = IniFile::from_str("[Map]\nSize=-1,-2,-40,-41\nLocalSize=-5,-6,-30,-31\n");

        let header = parse_header(&ini).expect("signed rectangle fields should parse");

        assert_eq!(header.width as i32, -40);
        assert_eq!(header.height as i32, -41);
        assert_eq!(header.local_left as i32, -5);
        assert_eq!(header.local_top as i32, -6);
        assert_eq!(header.local_width as i32, -30);
        assert_eq!(header.local_height as i32, -31);
    }

    #[test]
    fn gsi_04_01_rects_ignore_fields_after_the_first_four() {
        let ini = IniFile::from_str("[Map]\nSize=2,3,40,41,invalid\nLocalSize=5,6,30,31,invalid\n");

        let header = parse_header(&ini).expect("trailing fields do not invalidate a rectangle");

        assert_eq!(header.width, 40);
        assert_eq!(header.height, 41);
        assert_eq!(header.local_left, 5);
        assert_eq!(header.local_top, 6);
        assert_eq!(header.local_width, 30);
        assert_eq!(header.local_height, 31);
    }

    #[test]
    fn gsi_02_09_zero_header_terminates_before_later_records() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&iso_map_record(1, 0, 7, 2, 3));
        bytes.extend_from_slice(&[0; CELL_HEADER_SIZE]);
        bytes.extend_from_slice(&iso_map_record(2, 0, 8, 4, 5));

        let parsed = parse_iso_map_pack_records(&bytes).expect("sentinel terminates decoding");
        let cells = parsed.cells;

        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].rx, 1);
        assert_eq!(cells[0].tile_index, 7);
    }

    #[test]
    fn gsi_02_09_incomplete_header_is_an_error() {
        for actual in 1..CELL_HEADER_SIZE {
            let mut bytes = iso_map_record(1, 0, 1, 0, 0).to_vec();
            bytes.extend(std::iter::repeat_n(1, actual));
            assert!(matches!(
                parse_iso_map_pack_records(&bytes),
                Err(MapError::CellDataTruncated {
                    expected: CELL_HEADER_SIZE,
                    actual: got,
                }) if got == actual
            ));
        }
    }

    #[test]
    fn gsi_02_09_incomplete_non_sentinel_payload_is_an_error() {
        let mut bytes = vec![0; CELL_RECORD_SIZE - 1];
        bytes[0] = 1;

        assert!(matches!(
            parse_iso_map_pack_records(&bytes),
            Err(MapError::CellDataTruncated {
                expected: CELL_RECORD_SIZE,
                actual,
            }) if actual == CELL_RECORD_SIZE - 1
        ));
    }

    #[test]
    fn gsi_02_09_tile_index_preserves_raw_i32_values() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&iso_map_record(1, 0, 0x0000_FFFF, 0, 0));
        bytes.extend_from_slice(&iso_map_record(2, 0, -1, 0, 0));

        let parsed = parse_iso_map_pack_records(&bytes).expect("records are valid");
        let cells = parsed.cells;

        assert_eq!(cells[0].tile_index, 65_535);
        assert_eq!(cells[1].tile_index, -1);
    }

    #[test]
    fn gsi_04_01_isomap_parser_retains_ordered_raw_lookup_trace() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&iso_map_record(-1, 0, 101, 1, 2));
        bytes.extend_from_slice(&iso_map_record(-1, 1, 102, 3, 4));
        bytes.extend_from_slice(&iso_map_record(511, 511, 103, 5, 6));
        bytes.extend_from_slice(&[0; CELL_HEADER_SIZE]);

        let parsed = parse_iso_map_pack_records(&bytes).expect("complete records parse");

        assert_eq!(
            parsed.lookups,
            vec![
                IsoMapPackLookup {
                    raw_x: -1,
                    raw_y: 0,
                    canonical: None,
                },
                IsoMapPackLookup {
                    raw_x: -1,
                    raw_y: 1,
                    canonical: Some((511, 0)),
                },
                IsoMapPackLookup {
                    raw_x: 511,
                    raw_y: 511,
                    canonical: Some((511, 511)),
                },
            ]
        );
        assert_eq!(parsed.cells.len(), 2);
        assert_eq!((parsed.cells[0].rx, parsed.cells[0].ry), (511, 0));
        assert_eq!((parsed.cells[1].rx, parsed.cells[1].ry), (511, 511));
        assert_eq!(
            parsed
                .cells
                .iter()
                .map(|cell| cell.tile_index)
                .collect::<Vec<_>>(),
            vec![102, 103]
        );
    }

    #[test]
    fn test_missing_map_section() {
        let text: &str = "[General]\nKey=Value\n";
        let ini: IniFile = IniFile::from_str(text);
        let result = parse_header(&ini);
        assert!(result.is_err());
    }

    #[test]
    fn full_map_parse_decodes_its_preview_pack() {
        let bytes = full_map_with_preview(&format!(
            "[Preview]\nSize=0,0,2,1\n[PreviewPack]\n1={TWO_PIXEL_PREVIEW_PACK}\n"
        ));

        let map = MapFile::from_bytes(&bytes).expect("valid full map");
        let decoded = map.preview.decoded.expect("decoded map preview");

        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 1);
        assert_eq!(decoded.rgba, vec![1, 2, 3, 255, 4, 5, 6, 255]);
    }

    #[test]
    fn full_map_parse_keeps_invalid_preview_nonfatal() {
        let bytes =
            full_map_with_preview("[Preview]\nSize=0,0,2,1\n[PreviewPack]\n1=not valid base64!\n");

        let map = MapFile::from_bytes(&bytes).expect("preview failure is non-fatal");

        assert_eq!(map.preview.size, Some((2, 1)));
        assert!(map.preview.has_packed_preview);
        assert!(map.preview.decoded.is_none());
    }
}
