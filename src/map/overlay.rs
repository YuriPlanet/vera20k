//! Overlay and terrain object parsing from RA2 map files.
//!
//! RA2 maps have three overlay-related sections:
//! - `[OverlayPack]`: base64 → LCW compressed → 262,144 bytes (512×512 grid),
//!   or 1024×1024 on VERA's extended sizes (`docs/map-format.md`).
//!   Each byte is an overlay type ID (0xFF = no overlay).
//! - `[OverlayDataPack]`: same format. Each byte is a frame/value for the overlay
//!   (e.g., ore density 0-11, wall connection frame).
//! - `[Terrain]`: key = `ry * radix + rx` (128 before format4, otherwise1000),
//!   value = object name
//!   (e.g., "INTREE01", "CACTUS01").
//!
//! ## Dependency rules
//! - Part of map/ — depends on util/ (base64, lcw), rules/ (ini_parser).

// Outside data reaches this module: it refuses bad input, never panics.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable
    )
)]

use crate::map::cell_index::CellGrid;
use crate::rules::ini_parser::IniFile;
use crate::util::lcw;

/// Sentinel value meaning "no overlay at this cell".
const NO_OVERLAY: u8 = 0xFF;

/// Raw decoded `[OverlayPack]` body retained for the one native y/x reader.
///
/// The vector keeps its actual decoded length: `ReadMapOverlayPacks` attempts
/// one byte at every 512x512 coordinate and a short body's failed reads keep
/// the initialized no-overlay sentinel. Empty decoded identity data therefore
/// places no overlays even if a compressed section was present. The map's
/// cell grid sets the row stride: 512 on retail-size maps, 1024 on extended ones.
#[derive(Debug, Default)]
pub struct OverlayIdentityPack {
    bytes: Vec<u8>,
    grid: CellGrid,
}

impl OverlayIdentityPack {
    pub fn from_decoded(bytes: Vec<u8>, grid: CellGrid) -> Self {
        Self { bytes, grid }
    }

    pub fn has_positive_length(&self) -> bool {
        !self.bytes.is_empty()
    }

    pub fn decoded_len(&self) -> usize {
        self.bytes.len()
    }

    /// Return the decoded byte for one fixed-grid reader coordinate. `None`
    /// represents native's failed byte read and is not the `0xFF` sentinel.
    pub fn read_byte(&self, rx: u16, ry: u16) -> Option<u8> {
        read_pack_byte(&self.bytes, self.grid, rx, ry)
    }
}

/// One byte of a decoded pack at `(rx, ry)` under the grid's row stride.
fn read_pack_byte(bytes: &[u8], grid: CellGrid, rx: u16, ry: u16) -> Option<u8> {
    let stride = usize::from(grid.side());
    if usize::from(rx) >= stride || usize::from(ry) >= stride {
        return None;
    }
    bytes
        .get(usize::from(ry) * stride + usize::from(rx))
        .copied()
}

/// An overlay placed on a specific map cell.
///
/// Overlays include ore, gems, walls, fences, bridges, rocks, and other
/// decorative or gameplay objects that sit on top of terrain tiles.
#[derive(Debug, Clone, Copy)]
pub struct OverlayEntry {
    /// Isometric X coordinate.
    pub rx: u16,
    /// Isometric Y coordinate.
    pub ry: u16,
    /// Overlay type ID (index into the [OverlayTypes] registry in rules.ini).
    pub overlay_id: u8,
    /// Frame/value from OverlayDataPack (ore density, wall frame, etc.).
    pub frame: u8,
}

/// Full `[OverlayDataPack]` byte grid.
///
/// ReadMapOverlayPacks (0x5FD580..0x5FD589) starts the final per-cell
/// `cell+0x11E` overwrite when the packed section supplied compressed bytes.
/// A short first LCW frame can leave the decoded byte grid entirely empty.
#[derive(Debug, Clone)]
pub struct OverlayDataPack {
    bytes: Vec<u8>,
    present: bool,
    grid: CellGrid,
}

impl OverlayDataPack {
    pub fn from_decoded(bytes: Vec<u8>, grid: CellGrid) -> Self {
        let present = !bytes.is_empty();
        Self {
            bytes,
            present,
            grid,
        }
    }

    pub fn missing() -> Self {
        Self {
            bytes: Vec::new(),
            present: false,
            grid: CellGrid::Retail,
        }
    }

    pub fn is_present(&self) -> bool {
        self.present
    }

    pub fn has_positive_length(&self) -> bool {
        !self.bytes.is_empty()
    }

    #[cfg(test)]
    pub fn decoded_len(&self) -> usize {
        self.bytes.len()
    }

    /// Return the decoded byte for one fixed-grid reader coordinate. A short
    /// body produces `None`, matching the native failed-read path rather than
    /// fabricating a decoded zero byte.
    pub fn read_byte(&self, rx: u16, ry: u16) -> Option<u8> {
        read_pack_byte(&self.bytes, self.grid, rx, ry)
    }

    pub fn byte_at(&self, rx: u16, ry: u16) -> u8 {
        self.read_byte(rx, ry).unwrap_or(0)
    }

    /// Build a present data pack from per-cell frame values `(rx, ry, frame)`.
    ///
    /// Used by the map generator to emit an overlay-density grid alongside the
    /// overlay list; out-of-grid coordinates are ignored.
    pub fn from_cells(cells: impl IntoIterator<Item = (u16, u16, u8)>) -> Self {
        let cells: Vec<_> = cells.into_iter().collect();
        let grid = if cells.iter().any(|&(x, y, _)| x >= 512 || y >= 512) {
            CellGrid::Extended
        } else {
            CellGrid::Retail
        };
        let side = usize::from(grid.side());
        let mut bytes = vec![0u8; side * side];
        for (rx, ry, frame) in cells {
            if usize::from(rx) < side && usize::from(ry) < side {
                bytes[usize::from(ry) * side + usize::from(rx)] = frame;
            }
        }
        Self {
            bytes,
            present: true,
            grid,
        }
    }
}

impl Default for OverlayDataPack {
    fn default() -> Self {
        Self::missing()
    }
}

#[derive(Debug)]
pub struct ParsedOverlayPacks {
    pub identity: OverlayIdentityPack,
    pub entries: Vec<OverlayEntry>,
    pub data: OverlayDataPack,
}

/// A named terrain object placed on the map (trees, rocks).
///
/// These come from the [Terrain] INI section. Unlike overlays (which are
/// ID-indexed), terrain objects are named directly (e.g., "INTREE01").
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TerrainObject {
    /// Isometric X coordinate.
    pub rx: u16,
    /// Isometric Y coordinate.
    pub ry: u16,
    /// Object name (e.g., "INTREE01", "CACTUS01", "ROCK01").
    pub name: String,
}

/// Parse overlay entries and preserve the full overlay-data byte grid, laid
/// out at the map's cell-grid stride.
pub fn parse_overlay_packs(ini: &IniFile, grid: CellGrid) -> ParsedOverlayPacks {
    let identity = decode_pack_section(ini, "OverlayPack", grid)
        .map(|bytes| OverlayIdentityPack::from_decoded(bytes, grid))
        .unwrap_or_default();
    let data: OverlayDataPack = decode_pack_section(ini, "OverlayDataPack", grid)
        .map(|bytes| OverlayDataPack {
            bytes,
            present: true,
            grid,
        })
        .unwrap_or_else(OverlayDataPack::missing);

    let mut entries: Vec<OverlayEntry> = Vec::new();
    let stride = usize::from(grid.side());
    let max_idx: usize = identity.decoded_len().min(stride * stride);

    for idx in 0..max_idx {
        let overlay_id: u8 = identity.bytes[idx];
        if overlay_id == NO_OVERLAY {
            continue;
        }
        let rx: u16 = (idx % stride) as u16;
        let ry: u16 = (idx / stride) as u16;
        let frame: u8 = data.byte_at(rx, ry);

        entries.push(OverlayEntry {
            rx,
            ry,
            overlay_id,
            frame,
        });
    }

    log::info!("Parsed {} overlay entries from OverlayPack", entries.len());
    ParsedOverlayPacks {
        identity,
        entries,
        data,
    }
}

/// Parse terrain objects from the [Terrain] INI section.
///
/// Keys are decimal encoded positions. Terrain Read_Map_Section @0x0071CA70
/// selects radix128 for signed NewINIFormat<4, otherwise1000 (71CAE3..71CB44).
/// The caller supplies ReadBasic's retained format, whose native default is0.
/// Values are object names like "INTREE01", "CACTUS01", "ROCK01".
/// The writer `TerrainClass::Write_Map_Section @ 0x0071CB90` has no port: its
/// one caller, Save_Scenario_Map_File 0x00687CE0, is reached only from the
/// random-map dialog under the map-editor flag 0x00A8ED6B (0x00596A38..72).
pub fn parse_terrain_objects(ini: &IniFile, signed_new_ini_format: i32) -> Vec<TerrainObject> {
    let section = match ini.section("Terrain") {
        Some(s) => s,
        None => {
            log::trace!("No [Terrain] section in map");
            return Vec::new();
        }
    };

    let mut objects: Vec<TerrainObject> = Vec::new();
    let radix = if signed_new_ini_format < 4 { 128 } else { 1000 };

    // `TerrainClass__Read_Map_Section @ 0x0071CA70`: the cell is `atoi` of the
    // entry name, the type a 0x80-byte ReadString. IDIV/remainder are signed;
    // the resulting CellStruct retains only each word (71CAEF..71CB44).
    for key in section.keys() {
        let Some(name) = section.read_name(key, 0x80) else {
            continue;
        };
        // FindOrAllocate71E2D8 compares case-insensitively, but a new type's
        // AbstractTypeCtor41088D retains the input spelling. The selected
        // family member, not this reader, supplies the instance's identity.
        let name = name.to_string();
        let pos = crate::rules::ini_value::crt_atoi(key);

        let ry: u16 = (pos / radix) as u16;
        let rx: u16 = (pos % radix) as u16;

        objects.push(TerrainObject { rx, ry, name });
    }

    log::info!("Parsed {} terrain objects from [Terrain]", objects.len());
    objects
}

/// Decode a base64-encoded, LCW-compressed pack section (OverlayPack or OverlayDataPack).
fn decode_pack_section(ini: &IniFile, section_name: &str, grid: CellGrid) -> Option<Vec<u8>> {
    let section = ini.section(section_name)?;

    let b64_data = section.read_packed_text();

    if b64_data.is_empty() {
        return None;
    }

    // ReadMapOverlayPacks (0x5FD2E0): each of its two loops stops after
    // the last grid cell. A short pack retains only the available cells.
    let side = usize::from(grid.side());
    let limit = side * side;
    let mut data = Vec::new();
    let blocks = super::packed::decoded_blocks(
        &b64_data,
        limit,
        super::packed::EmptyFrame::Skip,
        lcw::lcw_decompress,
        |size, remaining| {
            // LCWStraw is constructed with an 8192-byte block (0x5FD3D2/
            // 0x5FD5D5). Its final block can extend beyond the last grid cell.
            let capacity = remaining.max(8192);
            if size > capacity {
                Err(lcw::LcwError::OutputOverflow {
                    needed: size,
                    capacity,
                })
            } else {
                Ok(())
            }
        },
    );
    for block in blocks {
        match block {
            Ok(block) => data.extend_from_slice(&block),
            Err(error) => {
                log::warn!("{section_name} LCW decompress failed: {error}");
                return None;
            }
        }
        if data.len() == limit {
            break;
        }
    }
    Some(data)
}

#[cfg(all(test, tests_map))]
mod tests {
    use super::*;
    use crate::util::base64;

    #[test]
    fn full_overlay_grid_stops_before_the_next_chunk() {
        let mut grid = vec![NO_OVERLAY; 512 * 512];
        *grid.last_mut().unwrap() = 7;
        grid.push(9); // Extra output in the final block, past (511, 511).
        let mut packed = vec![0; 4]; // An empty frame before real overlay data.
        packed.extend(lcw::compress_chunks(&grid, 8191));
        packed.extend_from_slice(&[7, 0, 1, 0]);
        let text = base64::base64_encode(&packed);
        let rows: String = text
            .as_bytes()
            .chunks(70)
            .enumerate()
            .map(|(i, value)| format!("{}={}\n", i + 1, std::str::from_utf8(value).unwrap()))
            .collect();
        let ini = IniFile::from_str(&format!("[OverlayPack]\n{rows}[OverlayDataPack]\n{rows}"));
        let packs = parse_overlay_packs(&ini, CellGrid::Retail);
        assert_eq!(packs.identity.decoded_len(), 512 * 512);
        assert_eq!(packs.identity.read_byte(511, 511), Some(7));
        assert_eq!(packs.data.read_byte(511, 511), Some(7));
    }

    #[test]
    fn overlay_pack_limits_cumulative_output_and_keeps_short_pack_reads() {
        // Seventeen 65535-byte RLE chunks exceed the extended 1024-square cap.
        // Keep each physical entry inside the INI reader's 127-byte limit.
        let chunk = "BQD///7///+A";
        let rows = format!("1={}\n2={}\n", chunk.repeat(10), chunk.repeat(7));
        let ini = IniFile::from_str(&format!("[OverlayPack]\n{rows}[OverlayDataPack]\n{rows}"));
        let rejected = parse_overlay_packs(&ini, CellGrid::Extended);
        assert!(!rejected.identity.has_positive_length());
        assert!(!rejected.data.is_present());
        // Five exceed the retail 512-square plane a retail-size map reads.
        let retail_over =
            IniFile::from_str(&format!("[OverlayPack]\n1={}\n", "BQD///7///+A".repeat(5)));
        let rejected = parse_overlay_packs(&retail_over, CellGrid::Retail);
        assert!(!rejected.identity.has_positive_length());

        let short = parse_overlay_packs(
            &IniFile::from_str("[OverlayPack]\n1=BQD///7///+A\n"),
            CellGrid::Retail,
        );
        assert_eq!(short.identity.decoded_len(), 65535);
        assert_eq!(short.identity.read_byte(0, 0), Some(NO_OVERLAY));
        assert_eq!(short.identity.read_byte(511, 127), None);
    }

    #[test]
    fn raw_identity_pack_distinguishes_sentinel_from_failed_short_read() {
        // A short retail body still has its native 512-byte row stride.
        let mut bytes = vec![NO_OVERLAY; 512 + 1];
        bytes[512] = 7;
        let pack = OverlayIdentityPack::from_decoded(bytes, CellGrid::Retail);

        assert!(pack.has_positive_length());
        assert_eq!(pack.read_byte(0, 0), Some(NO_OVERLAY));
        assert_eq!(pack.read_byte(0, 1), Some(7));
        assert_eq!(pack.read_byte(1, 1), None);
        assert_eq!(pack.read_byte(512, 0), None);
    }

    #[test]
    fn zero_length_pack_does_not_enter_signed_positive_body_gate() {
        let identity = OverlayIdentityPack::from_decoded(Vec::new(), CellGrid::Retail);
        let data = OverlayDataPack::from_decoded(Vec::new(), CellGrid::Retail);

        assert!(!identity.has_positive_length());
        assert_eq!(identity.read_byte(0, 0), None);
        assert!(!data.is_present());
        assert_eq!(data.byte_at(0, 0), 0);
    }

    #[test]
    fn raw_overlay_data_pack_preserves_failed_short_reads() {
        let data = OverlayDataPack::from_decoded(vec![7, 8], CellGrid::Retail);

        assert!(data.has_positive_length());
        assert_eq!(data.decoded_len(), 2);
        assert_eq!(data.read_byte(0, 0), Some(7));
        assert_eq!(data.read_byte(1, 0), Some(8));
        assert_eq!(data.read_byte(2, 0), None);
        assert_eq!(
            data.byte_at(2, 0),
            0,
            "legacy projection remains zero-fallback"
        );
    }

    #[test]
    fn test_parse_terrain_objects() {
        let text: &str = "\
[Terrain]
35042=INTREE01
35043=CACTUS01
72015=ROCK01
";
        let ini: IniFile = IniFile::from_str(text);
        let objects: Vec<TerrainObject> = parse_terrain_objects(&ini, 4);
        assert_eq!(objects.len(), 3);

        // Key 35042 → ry=35, rx=42
        let tree = objects.iter().find(|o| o.name == "INTREE01").unwrap();
        assert_eq!(tree.rx, 42);
        assert_eq!(tree.ry, 35);

        // Key 72015 → ry=72, rx=15
        let rock = objects.iter().find(|o| o.name == "ROCK01").unwrap();
        assert_eq!(rock.rx, 15);
        assert_eq!(rock.ry, 72);
    }
}

#[cfg(all(test, tests_map))]
mod large_map_tests {
    use super::*;
    #[test]
    fn large_map_overlay_stride_keeps_retail_and_extended_planes_distinct() {
        let mut retail = vec![255; 512 * 512];
        retail[400 * 512 + 300] = 102;
        assert_eq!(
            OverlayIdentityPack::from_decoded(retail, CellGrid::Retail).read_byte(300, 400),
            Some(102)
        );
        let mut large = vec![255; 1024 * 1024];
        large[600 * 1024 + 700] = 103;
        let pack = OverlayIdentityPack::from_decoded(large, CellGrid::Extended);
        assert_eq!(pack.read_byte(700, 600), Some(103));
        assert_eq!(pack.read_byte(188, 601), Some(255));
        let data = OverlayDataPack::from_cells([(700, 600, 11)]);
        assert_eq!(data.byte_at(700, 600), 11);
        assert_eq!(data.byte_at(188, 601), 0);
    }
}
