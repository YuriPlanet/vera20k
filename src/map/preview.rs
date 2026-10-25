//! Parser for `[Preview]` and `[PreviewPack]` map sections.
//!
//! This records preview metadata and decodes the packed thumbnail pixels used
//! by the skirmish shell on demand. GPU upload remains in the app/render layer.

use std::fmt;

use crate::rules::ini_parser::IniFile;
use crate::util::base64::base64_decode;
use crate::util::lzo::{LzoError, decompress_chunks};

/// Retained Scenario header input written by ReadStartPoints689D30. Map
/// discovery stores the parsed write, not a defaulted value: previewing a map
/// with no key (or failed hexadecimal conversion) must preserve the prior one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScenarioStartHeaderInput {
    coop_human_start_spots: Option<i32>,
}

impl ScenarioStartHeaderInput {
    pub fn from_ini(ini: &IniFile) -> Self {
        Self {
            coop_human_start_spots: ini
                .section("Header")
                .and_then(|section| section.read_int_value("NumCoopHumanStartSpots")),
        }
    }
}

/// Scenario+11E4: Set_Defaults683A40 seeds4; preview641F72 and scenario
/// load6874F8 both call the same retained ReadStartPoints reader. Shell visits
/// retain this owner; Clear_Scene685237 starts a fresh default before map load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ScenarioStartHeader {
    coop_human_start_spots: i32,
}

impl Default for ScenarioStartHeader {
    fn default() -> Self {
        Self {
            coop_human_start_spots: 4,
        }
    }
}

impl ScenarioStartHeader {
    pub fn apply(&mut self, input: ScenarioStartHeaderInput) {
        if let Some(value) = input.coop_human_start_spots {
            self.coop_human_start_spots = value;
        }
    }

    pub fn read_ini(&mut self, ini: &IniFile) {
        self.apply(ScenarioStartHeaderInput::from_ini(ini));
    }

    pub fn coop_human_start_spots(self) -> i32 {
        self.coop_human_start_spots
    }
}

/// Start slots the retained staging holds (`Scenario+0x1140`, 8 pairs).
pub const START_STAGING_SLOTS: usize = 8;

/// The retained Scenario start staging `+0x112C..+0x11DF` that the host's
/// `'W'` options string carries to its guests.
///
/// The host's random-map dialog writes it at Generate
/// (`ComputeScenarioHeaderFromMap 0x0058B830`); a guest's options parser
/// overwrites it from every `'W'` it receives (`0x005E3E96..0x005E40C6`).
/// Full_Init copies `cells` into waypoints 0..7 for a random map
/// (`0x006873E6..0x0068740E`) before House start assignment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScenarioStartStaging {
    /// `+0x112C` / `+0x1130`: the playfield's minimum screen X/60, Y/30.
    pub start_x: i32,
    pub start_y: i32,
    /// `+0x1134` / `+0x1138`: the spans of the same.
    pub width: i32,
    pub height: i32,
    /// `+0x113C`: staged start count.
    pub count: i32,
    /// `+0x1140`: start positions in the bounds' units.
    pub pairs: [(i32, i32); START_STAGING_SLOTS],
    /// `+0x11C0`: start waypoint cells; `(0, 0)` is the null cell (undefined).
    pub cells: [(i16, i16); START_STAGING_SLOTS],
}

impl ScenarioStartStaging {
    /// The `'W'` string `0x005E7EB0` formats: `"%c%d,%d,%d,%d,%d"` (`0x00831C18`)
    /// with width, height, start X, start Y and count, then
    /// `",%d,%d,%d,%d"` (`0x00831C08`) per slot with the pair and the cell.
    pub fn to_w_text(&self) -> String {
        let mut text = format!(
            "W{},{},{},{},{}",
            self.width, self.height, self.start_x, self.start_y, self.count
        );
        for (pair, cell) in self.pairs.iter().zip(&self.cells) {
            text.push_str(&format!(",{},{},{},{}", pair.0, pair.1, cell.0, cell.1));
        }
        text
    }

    /// The parser's `'W'` arm: `strtok(text + 1, ",")` and `atol` over 37
    /// tokens. It commits only when the 37th token exists
    /// (`0x005E3FE9`); nothing is clamped, and a cell keeps the low 16 bits
    /// of each value (`0x005E4041..0x005E4098`).
    pub fn parse_w_text(text: &str) -> Option<Self> {
        let body = text.strip_prefix('W')?;
        let values: Vec<i32> = crate::rules::ini_value::strtok(body, &[','])
            .take(5 + 4 * START_STAGING_SLOTS)
            .map(crate::rules::ini_value::crt_atoi)
            .collect();
        if values.len() < 5 + 4 * START_STAGING_SLOTS {
            return None;
        }
        let mut staging = Self {
            width: values[0],
            height: values[1],
            start_x: values[2],
            start_y: values[3],
            count: values[4],
            ..Self::default()
        };
        for (slot, fields) in values[5..].chunks_exact(4).enumerate() {
            staging.pairs[slot] = (fields[0], fields[1]);
            staging.cells[slot] = (fields[2] as i16, fields[3] as i16);
        }
        Some(staging)
    }

    /// The bounds `DrawStartPositions 0x00640710` projects the staged pairs
    /// through: none unless `1 <= count <= 8`. Native divides by the spans
    /// unguarded; a span that is not positive draws no markers here.
    pub fn preview_source_bounds(&self) -> Option<PreviewSourceBounds> {
        let count = usize::try_from(self.count).ok()?;
        if !(1..=START_STAGING_SLOTS).contains(&count) || self.width <= 0 || self.height <= 0 {
            return None;
        }
        Some(PreviewSourceBounds {
            origin_x: self.start_x,
            origin_y: self.start_y,
            width: self.width as u32,
            height: self.height as u32,
            start_points: self.pairs[..count]
                .iter()
                .map(|&(x, y)| PreviewStartPoint { x, y })
                .collect(),
        })
    }

    /// The defined start cells by slot, as Full_Init's copy leaves them:
    /// a slot holding the null cell stays undefined (`0x0068BD80`).
    pub fn defined_cells(&self) -> impl Iterator<Item = (usize, (i16, i16))> + '_ {
        self.cells
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, cell)| *cell != (0, 0))
    }
}

/// The colours the LAN host's preview upload asks for (`0x005E8340`).
pub const PALETTED_PREVIEW_COLORS: usize = 64;
/// `LZOPipe(0, 0x2000)` for `Preview.bin` (`0x005E837E`).
const PREVIEW_BIN_BLOCK_SIZE: usize = 0x2000;
// VERA resource ceiling for authored/network thumbnails, not a native format
// limit. Custom previews up to 2048x2048 remain supported (16 MiB decoded RGBA).
const MAX_PREVIEW_SIDE: u32 = 2048;
const MAX_PREVIEW_PIXELS: usize = MAX_PREVIEW_SIDE as usize * MAX_PREVIEW_SIDE as usize;
const MAX_PALETTED_PREVIEW_BYTES: usize = 12 + 256 * 2 + MAX_PREVIEW_PIXELS;

/// `CreatePalettedPreview 0x00642130`: the LAN host's preview, halved and
/// cut to `colors` 12-bit colours, as the blob
/// `u32 w, u32 h, u32 colors, colors × u16 0xRGB, w×h u8 indices`.
///
/// - Each pixel keeps the top four bits of each channel; the palette is the
///   most frequent such colours, the lowest code first on a tie, and stops
///   early when the image has fewer (`0x00642300..0x0064236F`).
/// - Each pixel takes the first entry at the least L1 distance, and the first
///   exact match at once (`0x0064246A..0x006424DC`).
///
/// The header always records `colors`. Native leaves the unfilled entries as
/// uninitialised heap; they are zero here, and no pixel can reach them,
/// because they exist only when every pixel has an exact match before them.
/// RESIDUAL: the halving is `DSurface::CopyFromPart`'s stretch blit, whose
/// sampling is not traced; this takes the source pixel at `x·w/(w/2)`.
pub fn create_paletted_preview(preview: &DecodedPreview, colors: usize) -> Vec<u8> {
    let (src_w, src_h) = (preview.width as usize, preview.height as usize);
    let (width, height) = (src_w / 2, src_h / 2);
    let mut pixels = Vec::with_capacity(width * height);
    let mut histogram = vec![0u32; 0x1000];
    for y in 0..height {
        for x in 0..width {
            let at = ((y * src_h / height) * src_w + x * src_w / width) * 4;
            let [r, g, b] = [0, 1, 2].map(|channel| preview.rgba[at + channel] & 0xF0);
            histogram[(usize::from(r) << 4) | usize::from(g) | usize::from(b >> 4)] += 1;
            pixels.push([r, g, b]);
        }
    }
    let mut palette = vec![0u16; colors];
    for entry in &mut palette {
        let mut best: Option<(usize, u32)> = None;
        for (code, count) in histogram.iter().copied().enumerate() {
            if count > best.map_or(0, |(_, most)| most) {
                best = Some((code, count));
            }
        }
        let Some((code, _)) = best else {
            break;
        };
        histogram[code] = 0;
        *entry = code as u16;
    }
    let mut blob = Vec::with_capacity(12 + 2 * colors + pixels.len());
    for field in [width, height, colors] {
        blob.extend_from_slice(&(field as u32).to_le_bytes());
    }
    for entry in &palette {
        blob.extend_from_slice(&entry.to_le_bytes());
    }
    for [r, g, b] in pixels {
        let mut best: Option<usize> = None;
        let mut least = 0xFFFF;
        for (index, &code) in palette.iter().enumerate() {
            let distance = (i32::from((code >> 4) & 0xF0) - i32::from(r)).abs()
                + (i32::from((code & 0xF) << 4) - i32::from(b)).abs()
                + (i32::from(code & 0xF0) - i32::from(g)).abs();
            if distance == 0 {
                best = Some(index);
                break;
            }
            if distance < least {
                least = distance;
                best = Some(index);
            }
        }
        blob.push(best.filter(|index| *index < 0x100).unwrap_or(0) as u8);
    }
    blob
}

/// `PreviewClass::CreatePreviewSurface 0x006425F0`: the blob's `w×h` image,
/// each index read as a palette entry `0xRGB` (`R = (v>>4)&0xF0`,
/// `G = v&0xF0`, `B = (v&0xF)<<4`). Native reads the entry at the index
/// without checking it against the colour count; an entry past the blob
/// reads as black here. A blob shorter than its header refuses.
pub fn paletted_preview_image(blob: &[u8]) -> Option<DecodedPreview> {
    let field = |at: usize| -> Option<usize> {
        Some(u32::from_le_bytes(blob.get(at..at + 4)?.try_into().ok()?) as usize)
    };
    let (width, height, colors) = (field(0)?, field(4)?, field(8)?);
    let rgba_len =
        expected_preview_rgba_len(width.try_into().ok()?, height.try_into().ok()?).ok()?;
    if colors > 256 {
        return None;
    }
    let indices_at = 12usize.checked_add(colors.checked_mul(2)?)?;
    let indices = blob.get(indices_at..indices_at.checked_add(rgba_len / 4)?)?;
    let mut rgba = Vec::with_capacity(rgba_len);
    for &index in indices {
        let at = 12 + 2 * usize::from(index);
        let code = blob
            .get(at..at + 2)
            .map_or(0, |bytes| u16::from_le_bytes([bytes[0], bytes[1]]));
        rgba.extend_from_slice(&[
            ((code >> 4) & 0xF0) as u8,
            (code & 0xF0) as u8,
            ((code & 0xF) << 4) as u8,
            0xFF,
        ]);
    }
    Some(DecodedPreview {
        width: width as u32,
        height: height as u32,
        rgba,
    })
}

/// `Preview.bin` as the host writes it (`0x005E83CA..0x005E8459`): the
/// blob's length, then the blob through `LZOPipe(0, 0x2000)`.
pub fn preview_bin_bytes(blob: &[u8]) -> Vec<u8> {
    let mut bytes = (blob.len() as u32).to_le_bytes().to_vec();
    bytes.extend(crate::util::lzo::compress_chunks(
        blob,
        PREVIEW_BIN_BLOCK_SIZE,
    ));
    bytes
}

/// The guest's read of `Preview.bin` (`0x005E7920`): dword 0 is the blob's
/// length, and `LZOStraw(1, 0x2000)` decodes the rest.
pub fn preview_bin_blob(bytes: &[u8]) -> Option<Vec<u8>> {
    let length = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    if !(12..=MAX_PALETTED_PREVIEW_BYTES).contains(&length) {
        return None;
    }
    let mut blob = decompress_chunks(&bytes[4..], length).ok()?;
    (blob.len() >= length).then(|| {
        blob.truncate(length);
        blob
    })
}

/// CPU RGBA thumbnail decoded from `[PreviewPack]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedPreview {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Parsed preview-related metadata from a map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PreviewSection {
    pub start_header: ScenarioStartHeaderInput,
    /// Size metadata from `[Preview] Size=` when present.
    pub size: Option<(u32, u32)>,
    /// True if `[PreviewPack]` exists and contains data.
    pub has_packed_preview: bool,
    /// Decoded CPU-side preview thumbnail when the map carries a valid pack.
    pub decoded: Option<DecodedPreview>,
}

/// Projected start point from `[Header] WaypointN`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewStartPoint {
    pub x: i32,
    pub y: i32,
}

/// Source rectangle inputs needed by the original preview marker projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewSourceBounds {
    pub origin_x: i32,
    pub origin_y: i32,
    pub width: u32,
    pub height: u32,
    pub start_points: Vec<PreviewStartPoint>,
}

/// Errors produced while decoding `[PreviewPack]` image data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewDecodeError {
    MissingPreviewPack,
    InvalidBase64(String),
    Lzo(String),
    PixelByteCount { expected: usize, actual: usize },
    PixelBufferTooLarge,
}

impl fmt::Display for PreviewDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingPreviewPack => write!(f, "missing or empty [PreviewPack]"),
            Self::InvalidBase64(err) => write!(f, "invalid PreviewPack base64: {err}"),
            Self::Lzo(err) => write!(f, "invalid PreviewPack LZO stream: {err}"),
            Self::PixelByteCount { expected, actual } => {
                write!(
                    f,
                    "PreviewPack byte count {actual} did not match expected {expected}"
                )
            }
            Self::PixelBufferTooLarge => {
                write!(f, "PreviewPack dimensions exceed supported bounds")
            }
        }
    }
}

impl std::error::Error for PreviewDecodeError {}

impl From<LzoError> for PreviewDecodeError {
    fn from(value: LzoError) -> Self {
        Self::Lzo(value.to_string())
    }
}

/// Parse preview metadata from map INI sections.
pub fn parse_preview_section(ini: &IniFile) -> PreviewSection {
    let size = preview_size(ini);

    let has_packed_preview = preview_pack_text(ini).is_some();

    PreviewSection {
        start_header: ScenarioStartHeaderInput::from_ini(ini),
        size,
        has_packed_preview,
        decoded: None,
    }
}

/// `[Preview] Size` is a ReadRect (`DecodePreviewPackToSurface @
/// 0x00641B00`) whose last two fields size the surface; a zero-sized rect has
/// none. Native overlays a partial rect on the previous preview's rect; Rust
/// overlays zeros.
fn preview_size(ini: &IniFile) -> Option<(u32, u32)> {
    let [_, _, width, height] = ini.section("Preview")?.read_rect("Size", [0; 4]);
    let size = (u32::try_from(width).ok()?, u32::try_from(height).ok()?);
    expected_preview_pixels(size.0, size.1).ok()?;
    Some(size)
}

fn preview_pack_text(ini: &IniFile) -> Option<String> {
    let text = ini.section("PreviewPack")?.read_packed_text();
    (!text.is_empty()).then_some(text)
}

pub(crate) fn expected_preview_pixels(
    width: u32,
    height: u32,
) -> Result<usize, PreviewDecodeError> {
    if !(1..=MAX_PREVIEW_SIDE).contains(&width) || !(1..=MAX_PREVIEW_SIDE).contains(&height) {
        return Err(PreviewDecodeError::PixelBufferTooLarge);
    }
    usize::try_from(width * height).map_err(|_| PreviewDecodeError::PixelBufferTooLarge)
}

fn expected_preview_rgb_len(width: u32, height: u32) -> Result<usize, PreviewDecodeError> {
    expected_preview_pixels(width, height)?
        .checked_mul(3)
        .ok_or(PreviewDecodeError::PixelBufferTooLarge)
}

fn expected_preview_rgba_len(width: u32, height: u32) -> Result<usize, PreviewDecodeError> {
    expected_preview_pixels(width, height)?
        .checked_mul(4)
        .ok_or(PreviewDecodeError::PixelBufferTooLarge)
}

/// `[PreviewPack]` pixels are stored in RGB order.
fn push_rgba_from_preview_pixel(out: &mut Vec<u8>, pixel: &[u8]) {
    out.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
}

/// Decode row-major 3-byte `[PreviewPack]` pixels into RGBA.
pub fn decode_preview_pack(
    pack_text: &str,
    width: u32,
    height: u32,
) -> Result<DecodedPreview, PreviewDecodeError> {
    let encoded = pack_text.trim();
    if encoded.is_empty() {
        return Err(PreviewDecodeError::MissingPreviewPack);
    }

    let expected = expected_preview_rgb_len(width, height)?;
    let compressed = base64_decode(encoded).map_err(PreviewDecodeError::InvalidBase64)?;
    let rgb = decompress_chunks(&compressed, expected)?;
    if rgb.len() != expected {
        return Err(PreviewDecodeError::PixelByteCount {
            expected,
            actual: rgb.len(),
        });
    }

    let mut rgba = Vec::with_capacity(expected_preview_rgba_len(width, height)?);
    for pixel in rgb.as_chunks::<3>().0 {
        push_rgba_from_preview_pixel(&mut rgba, pixel);
    }

    Ok(DecodedPreview {
        width,
        height,
        rgba,
    })
}

/// Decode the preview image from a full map INI.
///
/// This is intentionally separate from `parse_preview_section` so menu map
/// discovery can stay cheap even when the RA2 directory has many custom maps.
pub fn decode_preview_image_from_ini(
    ini: &IniFile,
) -> Result<Option<DecodedPreview>, PreviewDecodeError> {
    let Some((width, height)) = preview_size(ini) else {
        return Ok(None);
    };
    let Some(pack_text) = preview_pack_text(ini) else {
        return Ok(None);
    };
    decode_preview_pack(&pack_text, width, height).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn empty_preview_pack_is_not_counted() {
        let ini = IniFile::from_str("[PreviewPack]\n1=\n");
        let preview = parse_preview_section(&ini);
        assert_eq!(preview.size, None);
        assert!(!preview.has_packed_preview);
        assert_eq!(preview.decoded, None);
    }

    /// ReadRect scans a two-field value into the origin, leaving no size.
    #[test]
    fn parse_preview_size_two_fields_fill_the_origin() {
        let ini = IniFile::from_str("[Preview]\nSize=80,50\n");
        let preview = parse_preview_section(&ini);
        assert_eq!(preview.size, None);
    }

    #[test]
    fn preview_pack_text_preserves_source_order() {
        let ini = IniFile::from_str("[PreviewPack]\n2=BBB\n10=CCC\n1=AAA\n");
        assert_eq!(preview_pack_text(&ini).as_deref(), Some("BBBCCCAAA"));
    }

    #[test]
    fn decode_preview_pack_rejects_wrong_byte_count() {
        let err = decode_preview_pack("CgAGABcBAgMEBQYRAAA=", 3, 1).unwrap_err();
        assert_eq!(
            err,
            PreviewDecodeError::PixelByteCount {
                expected: 9,
                actual: 6,
            }
        );
    }

    #[test]
    fn preview_limits_dimensions_and_cumulative_decoded_bytes() {
        // Two independent three-byte RGB chunks cannot fit one declared pixel.
        assert!(matches!(
            decode_preview_pack("BwADABQBAgMRAAAHAAMAFAECAxEAAA==", 1, 1),
            Err(PreviewDecodeError::Lzo(_))
        ));
        // Dimensions are refused before even decoding the packed input.
        assert_eq!(
            decode_preview_pack("invalid base64", 2049, 1),
            Err(PreviewDecodeError::PixelBufferTooLarge)
        );
        let mut bin = 16u32.to_le_bytes().to_vec();
        bin.extend(crate::util::lzo::compress_chunks(&[0; 32], 16));
        assert!(preview_bin_blob(&bin).is_none());
    }

    #[test]
    fn parse_preview_section_leaves_valid_pack_lazy() {
        let ini =
            IniFile::from_str("[Preview]\nSize=0,0,2,1\n[PreviewPack]\n1=CgAGABcBAgMEBQYRAAA=\n");
        let preview = parse_preview_section(&ini);
        assert_eq!(preview.size, Some((2, 1)));
        assert!(preview.has_packed_preview);
        assert_eq!(preview.decoded, None);
    }
}

#[cfg(test)]
mod scenario_start_header_tests {
    use super::*;

    #[test]
    fn preview_visits_retain_header_until_scene_defaults_reset_it() {
        let mut header = ScenarioStartHeader::default();
        assert_eq!(header.coop_human_start_spots(), 4);
        for (source, expected) in [
            ("[Header]\nNumCoopHumanStartSpots=2", 2),
            ("[Header]\nStartX=1", 2),
            ("[Header]\nNumCoopHumanStartSpots=$no", 2),
            ("[Header]\nnumcoophumanstartspots=7", 2),
            ("[Header]\nNumCoopHumanStartSpots=garbage", 0),
            ("[Header]\nNumCoopHumanStartSpots=-3", -3),
        ] {
            let preview = parse_preview_section(&IniFile::from_str(source));
            header.apply(preview.start_header);
            assert_eq!(header.coop_human_start_spots(), expected, "{source}");
        }
        let mut fresh = ScenarioStartHeader::default();
        fresh.read_ini(&IniFile::from_str("[Header]\nStartX=1"));
        assert_eq!(fresh.coop_human_start_spots(), 4);
    }
}

#[cfg(test)]
mod paletted_preview_tests {
    use super::*;

    fn image(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 3]) -> DecodedPreview {
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let [r, g, b] = pixel(x, y);
                rgba.extend_from_slice(&[r, g, b, 0xFF]);
            }
        }
        DecodedPreview {
            width,
            height,
            rgba,
        }
    }

    #[test]
    fn the_blob_halves_ranks_colours_and_round_trips_through_preview_bin() {
        // Left half red-ish, right half blue-ish, one green pixel; the halved
        // 4x2 image samples even source pixels.
        let source = image(8, 4, |x, y| match (x, y) {
            (2, 2) => [0x00, 0xF7, 0x08],
            (x, _) if x < 4 => [0xF8, 0x1C, 0x07],
            _ => [0x07, 0x2F, 0xEE],
        });
        let blob = create_paletted_preview(&source, PALETTED_PREVIEW_COLORS);
        assert_eq!(blob.len(), 12 + 2 * 64 + 8);
        assert_eq!(&blob[..12], &[4, 0, 0, 0, 2, 0, 0, 0, 64, 0, 0, 0]);
        // Blue 0x02E has four pixels, red 0xF10 three and green one; the
        // unfilled entries are zero.
        let entry = |i: usize| u16::from_le_bytes([blob[12 + 2 * i], blob[13 + 2 * i]]);
        assert_eq!(
            [entry(0), entry(1), entry(2), entry(3)],
            [0x02E, 0xF10, 0x0F0, 0]
        );
        assert_eq!(&blob[140..], &[1, 1, 0, 0, 1, 2, 0, 0]);
        let decoded = paletted_preview_image(&blob).expect("decodes");
        assert_eq!((decoded.width, decoded.height), (4, 2));
        assert_eq!(&decoded.rgba[..4], &[0xF0, 0x10, 0x00, 0xFF]);
        assert_eq!(&decoded.rgba[20..24], &[0x00, 0xF0, 0x00, 0xFF]);
        assert_eq!(
            preview_bin_blob(&preview_bin_bytes(&blob)).as_deref(),
            Some(&blob[..])
        );
    }

    #[test]
    fn more_colours_than_the_palette_take_the_nearest() {
        let source = image(32, 2, |x, _| [(x as u8) << 3, 0, 0]);
        let blob = create_paletted_preview(&source, 4);
        // Sixteen reds at one pixel each: the four lowest codes win the tie.
        let entry = |i: usize| u16::from_le_bytes([blob[12 + 2 * i], blob[13 + 2 * i]]);
        assert_eq!(
            [entry(0), entry(1), entry(2), entry(3)],
            [0x000, 0x100, 0x200, 0x300]
        );
        // Red 0xF0 is nearest to 0x300's 0x30; the first of equals wins.
        assert_eq!(*blob.last().unwrap(), 3);
        assert!(paletted_preview_image(&blob[..blob.len() - 1]).is_none());
    }
}

#[cfg(test)]
mod start_staging_tests {
    use super::*;

    #[test]
    fn the_w_text_round_trips_and_needs_all_37_tokens() {
        let mut staging = ScenarioStartStaging {
            start_x: 3,
            start_y: -4,
            width: 120,
            height: 64,
            count: 2,
            ..ScenarioStartStaging::default()
        };
        staging.pairs[0] = (10, 20);
        staging.pairs[1] = (30, 40);
        staging.cells[0] = (55, 70);
        staging.cells[1] = (-1, 300);
        let text = staging.to_w_text();
        assert!(text.starts_with("W120,64,3,-4,2,10,20,55,70,30,40,-1,300,0,0,0,0"));
        assert_eq!(text.matches(',').count(), 36);
        assert_eq!(ScenarioStartStaging::parse_w_text(&text), Some(staging));
        let short = &text[..text.rfind(',').unwrap()];
        assert_eq!(ScenarioStartStaging::parse_w_text(short), None);
        assert_eq!(ScenarioStartStaging::parse_w_text(&text[1..]), None);
        assert_eq!(
            staging.defined_cells().collect::<Vec<_>>(),
            [(0, (55, 70)), (1, (-1, 300))]
        );
    }
}
