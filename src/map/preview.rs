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
            Self::PixelBufferTooLarge => write!(f, "PreviewPack dimensions overflowed"),
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
    (size.0 > 0 && size.1 > 0).then_some(size)
}

fn preview_pack_text(ini: &IniFile) -> Option<String> {
    let text = ini.section("PreviewPack")?.read_packed_text();
    (!text.is_empty()).then_some(text)
}

fn expected_preview_rgb_len(width: u32, height: u32) -> Result<usize, PreviewDecodeError> {
    let pixels = width
        .checked_mul(height)
        .ok_or(PreviewDecodeError::PixelBufferTooLarge)?;
    let bytes = pixels
        .checked_mul(3)
        .ok_or(PreviewDecodeError::PixelBufferTooLarge)?;
    usize::try_from(bytes).map_err(|_| PreviewDecodeError::PixelBufferTooLarge)
}

fn expected_preview_rgba_len(width: u32, height: u32) -> Result<usize, PreviewDecodeError> {
    let pixels = width
        .checked_mul(height)
        .ok_or(PreviewDecodeError::PixelBufferTooLarge)?;
    let bytes = pixels
        .checked_mul(4)
        .ok_or(PreviewDecodeError::PixelBufferTooLarge)?;
    usize::try_from(bytes).map_err(|_| PreviewDecodeError::PixelBufferTooLarge)
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

    let compressed = base64_decode(encoded).map_err(PreviewDecodeError::InvalidBase64)?;
    let rgb = decompress_chunks(&compressed)?;
    let expected = expected_preview_rgb_len(width, height)?;
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
        let err = decode_preview_pack("CgAGABcBAgMEBQYRAAA=", 1, 1).unwrap_err();
        assert_eq!(
            err,
            PreviewDecodeError::PixelByteCount {
                expected: 3,
                actual: 6,
            }
        );
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
