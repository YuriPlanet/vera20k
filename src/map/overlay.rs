//! Overlay and terrain object parsing from RA2 map files.
//!
//! RA2 maps have three overlay-related sections:
//! - `[OverlayPack]`: base64 → LCW compressed → 262,144 bytes (512×512 grid).
//!   Each byte is an overlay type ID (0xFF = no overlay).
//! - `[OverlayDataPack]`: same format. Each byte is a frame/value for the overlay
//!   (e.g., ore density 0-11, wall connection frame).
//! - `[Terrain]`: key = `ry * radix + rx` (128 before format4, otherwise1000),
//!   value = object name
//!   (e.g., "INTREE01", "CACTUS01").
//!
//! ## Dependency rules
//! - Part of map/ — depends on util/ (base64, lcw), rules/ (ini_parser).

use crate::rules::ini_parser::IniFile;
use crate::util::base64;
use crate::util::lcw;

/// Width of the overlay grid (RA2 maps are max 512×512 cells).
pub(crate) const OVERLAY_GRID_SIZE: usize = 512;

/// Total cells in the overlay grid (512 × 512 = 262,144).
pub(crate) const OVERLAY_TOTAL_CELLS: usize = OVERLAY_GRID_SIZE * OVERLAY_GRID_SIZE;

/// Sentinel value meaning "no overlay at this cell".
const NO_OVERLAY: u8 = 0xFF;

/// Raw decoded `[OverlayPack]` body retained for the one native y/x reader.
///
/// The vector keeps its actual decoded length: `ReadMapOverlayPacks` attempts
/// one byte at every 512x512 coordinate and a short body's failed reads keep
/// the initialized `0xFF` sentinel. A zero-length body is inactive under the
/// reader's signed-positive length gate.
#[derive(Debug, Default)]
pub struct OverlayIdentityPack {
    bytes: Vec<u8>,
}

impl OverlayIdentityPack {
    pub fn from_decoded(bytes: Vec<u8>) -> Self {
        Self { bytes }
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
        if usize::from(rx) >= OVERLAY_GRID_SIZE || usize::from(ry) >= OVERLAY_GRID_SIZE {
            return None;
        }
        self.bytes
            .get(usize::from(ry) * OVERLAY_GRID_SIZE + usize::from(rx))
            .copied()
    }
}

/// An overlay placed on a specific map cell.
///
/// Overlays include ore, gems, walls, fences, bridges, rocks, and other
/// decorative or gameplay objects that sit on top of terrain tiles.
#[derive(Debug, Clone)]
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
/// `present` matters because `gamemd.exe` only performs the final per-cell
/// `cell+0x11E` overwrite when the section exists and decodes.
#[derive(Debug, Clone)]
pub struct OverlayDataPack {
    bytes: Vec<u8>,
    present: bool,
}

impl OverlayDataPack {
    pub fn from_decoded(bytes: Vec<u8>) -> Self {
        let present = !bytes.is_empty();
        Self { bytes, present }
    }

    pub fn missing() -> Self {
        Self {
            bytes: Vec::new(),
            present: false,
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
        if usize::from(rx) >= OVERLAY_GRID_SIZE || usize::from(ry) >= OVERLAY_GRID_SIZE {
            return None;
        }
        self.bytes
            .get(usize::from(ry) * OVERLAY_GRID_SIZE + usize::from(rx))
            .copied()
    }

    pub fn byte_at(&self, rx: u16, ry: u16) -> u8 {
        self.read_byte(rx, ry).unwrap_or(0)
    }

    /// Build a present data pack from per-cell frame values `(rx, ry, frame)`.
    ///
    /// Used by the map generator to emit an overlay-density grid alongside the
    /// overlay list; out-of-grid coordinates are ignored.
    pub fn from_cells(cells: impl IntoIterator<Item = (u16, u16, u8)>) -> Self {
        let mut bytes = vec![0u8; OVERLAY_TOTAL_CELLS];
        for (rx, ry, frame) in cells {
            if (rx as usize) < OVERLAY_GRID_SIZE && (ry as usize) < OVERLAY_GRID_SIZE {
                bytes[ry as usize * OVERLAY_GRID_SIZE + rx as usize] = frame;
            }
        }
        Self {
            bytes,
            present: true,
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

/// Parse overlay entries and preserve the full overlay-data byte grid.
pub fn parse_overlay_packs(ini: &IniFile) -> ParsedOverlayPacks {
    let identity = decode_pack_section(ini, "OverlayPack")
        .map(OverlayIdentityPack::from_decoded)
        .unwrap_or_default();
    let data: OverlayDataPack = decode_pack_section(ini, "OverlayDataPack")
        .map(OverlayDataPack::from_decoded)
        .unwrap_or_else(OverlayDataPack::missing);

    let mut entries: Vec<OverlayEntry> = Vec::new();
    let max_idx: usize = identity.decoded_len().min(OVERLAY_TOTAL_CELLS);

    for idx in 0..max_idx {
        let overlay_id: u8 = identity.bytes[idx];
        if overlay_id == NO_OVERLAY {
            continue;
        }
        let rx: u16 = (idx % OVERLAY_GRID_SIZE) as u16;
        let ry: u16 = (idx / OVERLAY_GRID_SIZE) as u16;
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
///
/// Concatenates all numbered key values, base64 decodes, then LCW decompresses.
fn decode_pack_section(ini: &IniFile, section_name: &str) -> Option<Vec<u8>> {
    let section = ini.section(section_name)?;

    let b64_data = section.read_packed_text();

    if b64_data.is_empty() {
        return None;
    }

    let compressed: Vec<u8> = match base64::base64_decode(&b64_data) {
        Ok(d) => d,
        Err(e) => {
            log::warn!("{} base64 decode failed: {}", section_name, e);
            return None;
        }
    };

    // ReadMapOverlayPacks 0x005FD2E0 consumes one byte per fixed-grid cell.
    // Reject excess decoded data before allocating it; short packs retain the
    // existing failed-read semantics in OverlayIdentityPack/OverlayDataPack.
    match lcw::decompress_chunks(&compressed, OVERLAY_TOTAL_CELLS) {
        Ok(data) => {
            log::info!(
                "{}: {} compressed → {} decompressed bytes",
                section_name,
                compressed.len(),
                data.len()
            );
            Some(data)
        }
        Err(e) => {
            log::warn!("{} LCW decompress failed: {}", section_name, e);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_pack_limits_cumulative_output_and_keeps_short_pack_reads() {
        // Four 65535-byte RLE chunks plus five more bytes exceed 512x512.
        let packed = "BQD///7///+ABQD///7///+ABQD///7///+ABQD///7///+ABQAFAP4FAP+A";
        let ini = IniFile::from_str(&format!(
            "[OverlayPack]\n1={packed}\n[OverlayDataPack]\n1={packed}\n"
        ));
        let rejected = parse_overlay_packs(&ini);
        assert!(!rejected.identity.has_positive_length());
        assert!(!rejected.data.has_positive_length());

        let short = parse_overlay_packs(&IniFile::from_str("[OverlayPack]\n1=BQD///7///+A\n"));
        assert_eq!(short.identity.decoded_len(), 65535);
        assert_eq!(short.identity.read_byte(0, 0), Some(NO_OVERLAY));
        assert_eq!(short.identity.read_byte(511, 127), None);
    }

    #[test]
    fn raw_identity_pack_distinguishes_sentinel_from_failed_short_read() {
        let mut bytes = vec![NO_OVERLAY; OVERLAY_GRID_SIZE + 1];
        bytes[OVERLAY_GRID_SIZE] = 7;
        let pack = OverlayIdentityPack::from_decoded(bytes);

        assert!(pack.has_positive_length());
        assert_eq!(pack.read_byte(0, 0), Some(NO_OVERLAY));
        assert_eq!(pack.read_byte(0, 1), Some(7));
        assert_eq!(pack.read_byte(1, 1), None);
        assert_eq!(pack.read_byte(512, 0), None);
    }

    #[test]
    fn zero_length_pack_does_not_enter_signed_positive_body_gate() {
        let identity = OverlayIdentityPack::from_decoded(Vec::new());
        let data = OverlayDataPack::from_decoded(Vec::new());

        assert!(!identity.has_positive_length());
        assert_eq!(identity.read_byte(0, 0), None);
        assert!(!data.is_present());
        assert_eq!(data.byte_at(0, 0), 0);
    }

    #[test]
    fn raw_overlay_data_pack_preserves_failed_short_reads() {
        let data = OverlayDataPack::from_decoded(vec![7, 8]);

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
