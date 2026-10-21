//! Parser for RA2 .shp(ts) sprite files.
//!
//! SHP files contain multiple frames of 8-bit indexed color data.
//! Used for buildings, infantry, animations, cameo icons, and UI elements.
//! Each pixel is a palette index (0 = transparent).
//!
//! ## Format (Tiberian Sun / Red Alert 2 variant, "SHP(TS)")
//!
//! ### File header (8 bytes):
//! ```text
//! u16: zero (always 0 — distinguishes from older SHP format)
//! u16: width  (max frame width in pixels)
//! u16: height (max frame height in pixels)
//! u16: frame_count (number of frames in this file)
//! ```
//!
//! ### Per-frame header (24 bytes each, frame_count total):
//! ```text
//! u16: frame_x      — X offset within the full sprite bounds
//! u16: frame_y      — Y offset within the full sprite bounds
//! u16: frame_width  — width of this specific frame's pixel data
//! u16: frame_height — height of this specific frame's pixel data
//! u08: format       — bit 1 selects row-framed RLE-Zero; clear means raw
//! u24: padding/reserved
//! u32: zero/reserved
//! u32: data_offset  — byte offset from file start to this frame's pixel data
//! ```
//!
//! ### Frame pixel data:
//! - If format bit 1 is clear: raw palette indices, width * height bytes
//! - If format bit 1 is set: see shp_decode.rs for row/RLE-Zero details.
//!
//! ## Dependency rules
//! - Part of assets/ — no dependencies on game modules.
//! - Uses util/read_helpers for binary reading, assets/shp_decode for RLE decompression.

use crate::assets::error::AssetError;
use crate::assets::pal_file::Palette;
use crate::assets::shp_decode::decode_rle_frame;
use crate::util::read_helpers::{read_u16_le, read_u32_le};

/// Byte-8 dispatch bit used by the active YR SHP draw path.
const FORMAT_RLE_ZERO_BIT: u8 = 0x02;

/// A parsed SHP sprite file containing one or more frames.
///
/// Frames are stored as palette indices (u8). Convert to RGBA
/// using `frame_to_rgba()` with a palette for rendering.
#[derive(Debug)]
pub struct ShpFile {
    /// Maximum width across all frames (from file header).
    pub width: u16,
    /// Maximum height across all frames (from file header).
    pub height: u16,
    /// The individual sprite frames.
    pub frames: Vec<ShpFrame>,
}

/// A single frame from an SHP file.
///
/// Each pixel is a palette index. Index 0 = transparent.
/// The frame may be smaller than the file's overall width/height,
/// positioned at (frame_x, frame_y) within the full bounds.
#[derive(Debug)]
pub struct ShpFrame {
    /// X offset of this frame within the full sprite bounds.
    pub frame_x: u16,
    /// Y offset of this frame within the full sprite bounds.
    pub frame_y: u16,
    /// Width of this frame's pixel data.
    pub frame_width: u16,
    /// Height of this frame's pixel data.
    pub frame_height: u16,
    /// Original byte-8 format bitfield from this frame's header.
    pub format: u8,
    /// Per-frame radar/minimap colour baked into the frame header.
    ///
    /// Ore and gem overlays carry one of these per growth stage, and the engine
    /// indexes them by the cell's density byte to colour minimap and preview
    /// pixels — the frame's artwork is not sampled for that.
    pub radar_color: [u8; 3],
    /// Decoded pixel data (palette indices). Length = frame_width * frame_height.
    /// Index 0 means transparent.
    pub pixels: Vec<u8>,
}

impl ShpFile {
    /// Read the declared frame count without decoding any frame pixels.
    ///
    /// This validates the SHP(TS) file marker and verifies that the complete
    /// declared frame-header table is present, matching the header validation
    /// performed by [`Self::from_bytes`]. Frame data offsets and payloads are
    /// deliberately left to the full parser.
    pub fn frame_count_from_bytes(data: &[u8]) -> Result<u16, AssetError> {
        if data.len() < 8 {
            return Err(AssetError::InvalidShpHeader {
                reason: format!(
                    "File too small for header: {} bytes (need at least 8)",
                    data.len()
                ),
            });
        }

        // Bytes 0-1: should be zero (distinguishes SHP(TS) from older SHP format).
        let zero = read_u16_le(data, 0);
        if zero != 0 {
            return Err(AssetError::InvalidShpHeader {
                reason: format!(
                    "First two bytes should be 0 for SHP(TS) format, got {}",
                    zero
                ),
            });
        }

        let frame_count = read_u16_le(data, 6);
        let headers_end = 8 + usize::from(frame_count) * 24;
        if data.len() < headers_end {
            return Err(AssetError::InvalidShpHeader {
                reason: format!(
                    "File too small for {} frame headers: {} bytes (need {})",
                    frame_count,
                    data.len(),
                    headers_end
                ),
            });
        }

        Ok(frame_count)
    }

    /// Parse an SHP file from raw bytes.
    ///
    /// Reads the file header, all frame headers, then decodes each frame's
    /// pixel data (handling both raw and RLE-compressed formats).
    pub fn from_bytes(data: &[u8]) -> Result<Self, AssetError> {
        let frame_count = Self::frame_count_from_bytes(data)?;
        let width: u16 = read_u16_le(data, 2);
        let height: u16 = read_u16_le(data, 4);
        let mut frames: Vec<ShpFrame> = Vec::with_capacity(usize::from(frame_count));
        for i in 0..usize::from(frame_count) {
            frames.push(Self::frame_at(data, i)?);
        }
        Ok(ShpFile {
            width,
            height,
            frames,
        })
    }

    /// Decode frame `index`'s palette indices from the file's raw bytes, as
    /// [`Self::from_bytes`] decodes each frame. It reads only that frame, so
    /// it can succeed on a file whose other frames `from_bytes` rejects.
    pub fn decode_frame(data: &[u8], index: usize) -> Result<Vec<u8>, AssetError> {
        let frame_count = Self::frame_count_from_bytes(data)?;
        if index >= usize::from(frame_count) {
            return Err(AssetError::ShpFrameOutOfRange {
                index: index as u16,
                count: frame_count,
            });
        }
        Ok(Self::frame_at(data, index)?.pixels)
    }

    /// `ShapeFile::GetFrameData @ 69E740`: encoded bytes from this frame's
    /// file offset onward, without RLE decoding or truncation at the next
    /// frame. The optional depth surface in CC_Draw_Shape uses this view.
    pub fn encoded_frame_data(data: &[u8], index: usize) -> Result<&[u8], AssetError> {
        let count = Self::frame_count_from_bytes(data)?;
        if index >= usize::from(count) {
            return Err(AssetError::ShpFrameOutOfRange {
                index: index as u16,
                count,
            });
        }
        Self::frame_data_at(data, index)
    }

    fn frame_data_at(data: &[u8], i: usize) -> Result<&[u8], AssetError> {
        let start = read_u32_le(data, 8 + i * 24 + 20) as usize;
        if start == 0 {
            return Err(AssetError::ParseError {
                format: "SHP".to_string(),
                detail: format!("Frame {} is nonempty but has a zero data offset", i),
            });
        }
        data.get(start..)
            .filter(|bytes| !bytes.is_empty())
            .ok_or_else(|| AssetError::ParseError {
                format: "SHP".to_string(),
                detail: format!(
                    "Frame {} data offset {} is past end of file ({})",
                    i,
                    start,
                    data.len()
                ),
            })
    }

    /// Frame `i`'s header and decoded pixels; the caller checked that the
    /// header table holds it.
    fn frame_at(data: &[u8], i: usize) -> Result<ShpFrame, AssetError> {
        let hdr_offset: usize = 8 + i * 24;

        let frame_x: u16 = read_u16_le(data, hdr_offset);
        let frame_y: u16 = read_u16_le(data, hdr_offset + 2);
        let frame_width: u16 = read_u16_le(data, hdr_offset + 4);
        let frame_height: u16 = read_u16_le(data, hdr_offset + 6);
        let format: u8 = data[hdr_offset + 8];
        // Bytes 9-11: padding/reserved
        // Bytes 12-14: radar minimap colour (R, G, B); byte 15 unused
        // Bytes 16-19: reserved (always 0)
        // Bytes 20-23: data_offset (absolute file offset to this frame's pixel data)
        let radar_color: [u8; 3] = [
            data[hdr_offset + 12],
            data[hdr_offset + 13],
            data[hdr_offset + 14],
        ];

        // A frame with zero dimensions has no pixel data (empty frame).
        if frame_width == 0 || frame_height == 0 {
            return Ok(ShpFrame {
                frame_x,
                frame_y,
                frame_width,
                frame_height,
                format,
                radar_color,
                pixels: Vec::new(),
            });
        }

        let pixel_count: usize = frame_width as usize * frame_height as usize;
        let frame_slice = Self::frame_data_at(data, i)?;
        let frame_data_start = data.len() - frame_slice.len();
        let pixels: Vec<u8> = if format & FORMAT_RLE_ZERO_BIT != 0 {
            decode_rle_frame(frame_slice, frame_width as usize, frame_height as usize)?
        } else {
            let end = frame_data_start.checked_add(pixel_count).ok_or_else(|| {
                AssetError::ParseError {
                    format: "SHP".to_string(),
                    detail: format!("Frame {} raw data extent overflows address space", i),
                }
            })?;
            if end > data.len() {
                return Err(AssetError::ParseError {
                    format: "SHP".to_string(),
                    detail: format!(
                        "Frame {} raw data extends past end of file ({} > {})",
                        i,
                        end,
                        data.len()
                    ),
                });
            }
            data[frame_data_start..end].to_vec()
        };

        Ok(ShpFrame {
            frame_x,
            frame_y,
            frame_width,
            frame_height,
            format,
            radar_color,
            pixels,
        })
    }

    /// Convert a frame's palette-indexed pixels to RGBA using the given palette.
    ///
    /// Returns a Vec of width * height * 4 bytes (RGBA): the draw of
    /// BlitTransXlat<unsigned short>::BlitForward `0x004912B0` and
    /// RLEBlitTransXlat<unsigned short>::Blit `0x004960A0`, which skip source
    /// index 0 and write the converter entry of every other index, whatever
    /// colour it holds. CC_Draw_Shape `0x004AED70` gets them from
    /// ConvertClass::Select_Blitter `0x00490B90` (uncompressed frames) and
    /// Select_RLE_Blitter `0x00490E50` (compressed frames) when the flags set
    /// no darken (1), lucent (6), remap (0x10), Z (0x3000, 0x4000), alpha
    /// (0x800) or write-alpha (0x40, 0x100, 0x8000) bit, as flags 0 and 0x400
    /// do. Other draws differ: flag 0x20 on an uncompressed frame picks
    /// BlitPlainXlat `0x004924C0`, which draws index 0 as well, and the
    /// darken, lucent and remap blitters draw differently.
    pub fn frame_to_rgba(
        &self,
        frame_index: usize,
        palette: &Palette,
    ) -> Result<Vec<u8>, AssetError> {
        if frame_index >= self.frames.len() {
            return Err(AssetError::ShpFrameOutOfRange {
                index: frame_index as u16,
                count: self.frames.len() as u16,
            });
        }

        let frame: &ShpFrame = &self.frames[frame_index];
        let pixel_count: usize = frame.frame_width as usize * frame.frame_height as usize;
        let mut rgba: Vec<u8> = Vec::with_capacity(pixel_count * 4);

        for &palette_index in &frame.pixels {
            let color = palette.color(palette_index);
            rgba.push(color.r);
            rgba.push(color.g);
            rgba.push(color.b);
            rgba.push(if palette_index == 0 { 0 } else { 255 });
        }

        Ok(rgba)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PIN — index 0 is transparent by BLITTER contract, not palette content.
    ///
    /// Retail palettes carry no alpha; the loaders preserve that (index 0
    /// opaque in the `Palette`). gamemd's transparent SHP blitters skip
    /// index-0 pixels unconditionally, so frame conversion must key on the
    /// INDEX. When this keyed on `color.a` instead, every sprite in the game
    /// grew an opaque palette-0 (blue) box the moment palette loading went
    /// byte-faithful — invisible to the whole suite until a live run.
    #[test]
    fn frame_to_rgba_bakes_index_zero_transparent_even_when_palette_has_no_alpha() {
        let shp = ShpFile::from_bytes(&make_test_shp_raw()).expect("parse");
        // 768 raw bytes: EVERY entry opaque, index 0 included.
        let pal = Palette::from_bytes(&vec![21u8; 768]).expect("palette");
        assert_eq!(
            pal.color(0).a,
            255,
            "fixture guard: palette carries no alpha"
        );

        // Test frame pixels are [1, 2, 3, 0] — the last one is index 0.
        let rgba = shp.frame_to_rgba(0, &pal).expect("convert");
        assert_eq!(rgba[3], 255, "index 1 stays opaque");
        assert_eq!(rgba[7], 255, "index 2 stays opaque");
        assert_eq!(rgba[11], 255, "index 3 stays opaque");
        assert_eq!(
            rgba[15], 0,
            "index 0 must bake transparent regardless of palette alpha"
        );
    }

    /// Build a minimal valid SHP file with one uncompressed 2x2 frame.
    fn make_test_shp_raw() -> Vec<u8> {
        let mut data: Vec<u8> = Vec::new();

        // File header (8 bytes): zero=0, width=2, height=2, frame_count=1
        data.extend_from_slice(&0u16.to_le_bytes()); // zero
        data.extend_from_slice(&2u16.to_le_bytes()); // width
        data.extend_from_slice(&2u16.to_le_bytes()); // height
        data.extend_from_slice(&1u16.to_le_bytes()); // frame_count

        // Frame header (24 bytes):
        // Byte layout: x(2) y(2) w(2) h(2) format(1) padding(11) data_offset(4)
        let data_offset: u32 = 8 + 24; // right after the header
        data.extend_from_slice(&0u16.to_le_bytes()); // +0: frame_x
        data.extend_from_slice(&0u16.to_le_bytes()); // +2: frame_y
        data.extend_from_slice(&2u16.to_le_bytes()); // +4: frame_width
        data.extend_from_slice(&2u16.to_le_bytes()); // +6: frame_height
        data.push(0x00); // +8: format (0 = raw)
        data.extend_from_slice(&[0u8; 3]); // +9: padding (3 bytes)
        data.extend_from_slice(&[0x40, 0x80, 0xC0]); // +12: radar colour R,G,B
        data.extend_from_slice(&[0u8; 5]); // +15: unused + reserved
        data.extend_from_slice(&data_offset.to_le_bytes()); // +20: data_offset

        // Pixel data: 2x2 = 4 bytes (palette indices: 1, 2, 3, 0)
        data.extend_from_slice(&[1, 2, 3, 0]);

        data
    }

    fn make_test_shp_with_format(format: u8, payload: &[u8]) -> Vec<u8> {
        let mut data = make_test_shp_raw();
        data[8 + 8] = format;
        data.truncate(8 + 24);
        data.extend_from_slice(payload);
        data
    }

    #[test]
    fn encoded_frame_view_preserves_rle_and_following_file_bytes() {
        let payload = [4, 0, 0, 2, 4, 0, 7, 8, 0xF1, 0x82];
        let data = make_test_shp_with_format(3, &payload);
        assert_eq!(ShpFile::decode_frame(&data, 0).unwrap(), [0, 0, 7, 8]);
        assert_eq!(ShpFile::encoded_frame_data(&data, 0).unwrap(), payload);
        assert!(ShpFile::encoded_frame_data(&data, 1).is_err());
        let mut invalid = data;
        invalid[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(ShpFile::encoded_frame_data(&invalid, 0).is_err());
    }

    #[test]
    fn frame_count_reader_validates_headers_without_decoding_pixels() {
        let mut data = make_test_shp_raw();
        data.truncate(8 + 24);

        assert_eq!(ShpFile::frame_count_from_bytes(&data).unwrap(), 1);
        assert!(
            ShpFile::from_bytes(&data).is_err(),
            "the full parser must still validate the missing pixel payload"
        );
    }

    #[test]
    fn frame_count_reader_rejects_a_truncated_declared_header_table() {
        let mut data = make_test_shp_raw();
        data[6..8].copy_from_slice(&2u16.to_le_bytes());

        let error = ShpFile::frame_count_from_bytes(&data).unwrap_err();
        assert!(matches!(&error, AssetError::InvalidShpHeader { .. }));
        assert_eq!(
            error.to_string(),
            ShpFile::from_bytes(&data).unwrap_err().to_string(),
            "the cheap reader and full parser must preserve header error semantics"
        );
    }

    #[test]
    fn format_two_and_three_share_the_rle_zero_grammar() {
        let payload = [
            4, 0, 0, 2, // row 0: two transparent pixels
            4, 0, 7, 8, // row 1: two literals
        ];
        let format_two =
            ShpFile::from_bytes(&make_test_shp_with_format(2, &payload)).expect("format 2 row/RLE");
        let format_three =
            ShpFile::from_bytes(&make_test_shp_with_format(3, &payload)).expect("format 3 row/RLE");

        assert_eq!(format_two.frames[0].format, 2);
        assert_eq!(format_three.frames[0].format, 3);
        assert_eq!(format_two.frames[0].pixels, vec![0, 0, 7, 8]);
        assert_eq!(format_two.frames[0].pixels, format_three.frames[0].pixels);
    }

    #[test]
    fn format_dispatch_uses_bit_one_for_extended_rows() {
        let extended_payload = [
            4, 0, 0, 2, // row 0
            4, 0, 5, 6, // row 1
        ];
        let extended = ShpFile::from_bytes(&make_test_shp_with_format(6, &extended_payload))
            .expect("format 6 has bit 1 set");
        assert_eq!(extended.frames[0].pixels, vec![0, 0, 5, 6]);

        let raw = ShpFile::from_bytes(&make_test_shp_with_format(4, &[0, 2, 5, 9]))
            .expect("format 4 has bit 1 clear");
        assert_eq!(raw.frames[0].format, 4);
        assert_eq!(raw.frames[0].pixels, vec![0, 2, 5, 9]);
    }

    #[test]
    fn empty_bitfield_formats_keep_metadata_with_zero_data_offset() {
        for format in [4, 204] {
            let mut data = make_test_shp_raw();
            data[8 + 4..8 + 8].fill(0); // zero frame width and height
            data[8 + 8] = format;
            data[8 + 20..8 + 24].fill(0); // native empty-frame null data
            data.truncate(8 + 24);

            let shp = ShpFile::from_bytes(&data).expect("empty metadata frame");
            let frame = &shp.frames[0];
            assert_eq!(frame.format, format);
            assert_eq!(frame.radar_color, [0x40, 0x80, 0xC0]);
            assert_eq!((frame.frame_width, frame.frame_height), (0, 0));
            assert!(frame.pixels.is_empty());
        }
    }

    #[test]
    fn a_frame_decodes_alone_and_an_index_past_the_last_is_out_of_range() {
        // The raw 2x2 test frame, then a frame whose data starts past the end.
        let one = make_test_shp_raw();
        let mut data = one[..8].to_vec();
        data[6..8].copy_from_slice(&2u16.to_le_bytes());
        let mut first = one[8..32].to_vec();
        first[20..24].copy_from_slice(&56u32.to_le_bytes());
        let mut second = first.clone();
        second[20..24].copy_from_slice(&1000u32.to_le_bytes());
        data.extend(first);
        data.extend(second);
        data.extend_from_slice(&one[32..]);

        assert!(
            ShpFile::from_bytes(&data).is_err(),
            "frame 1 fails the file"
        );
        assert_eq!(ShpFile::decode_frame(&data, 0).unwrap(), vec![1, 2, 3, 0]);
        assert!(matches!(
            ShpFile::decode_frame(&data, 2),
            Err(AssetError::ShpFrameOutOfRange { index: 2, count: 2 })
        ));
    }

    #[test]
    fn nonempty_frame_with_zero_data_offset_errors() {
        let mut data = make_test_shp_raw();
        data[8 + 20..8 + 24].fill(0);
        assert!(ShpFile::from_bytes(&data).is_err());
    }

    #[test]
    fn raw_frame_requires_its_exact_visible_extent_inside_the_file() {
        let mut data = make_test_shp_raw();
        data.pop();
        assert!(ShpFile::from_bytes(&data).is_err());
    }

    #[test]
    fn gsi_02_13_frame_to_rgba_uses_native_shifted_palette_channels() {
        let data: Vec<u8> = make_test_shp_raw();
        let shp: ShpFile = ShpFile::from_bytes(&data).expect("Should parse");

        // Create a palette where index 1 = red, index 2 = green, index 3 = blue
        let mut pal_data: Vec<u8> = vec![0u8; 768];
        pal_data[3] = 63; // Index 1: R=63 (max red)
        pal_data[7] = 63; // Index 2: G=63 (max green)
        pal_data[11] = 63; // Index 3: B=63 (max blue)

        let palette: Palette = Palette::from_bytes(&pal_data).expect("Should parse palette");
        let rgba: Vec<u8> = shp.frame_to_rgba(0, &palette).expect("Should convert");

        // 2x2 * 4 bytes = 16 bytes
        assert_eq!(rgba.len(), 16);
        // Pixel 0 (index 1): native-shifted red (252, 0, 0, 255)
        assert_eq!(rgba[0], 252); // R
        assert_eq!(rgba[1], 0); // G
        assert_eq!(rgba[3], 255); // A (opaque)
        // Pixel 3 (index 0): transparent
        assert_eq!(rgba[15], 0); // A (transparent)
    }

    /// BlitTransXlat<unsigned short>::BlitForward `0x004912B0` and
    /// RLEBlitTransXlat<unsigned short>::Blit `0x004960A0` skip only source
    /// index 0 and write the converter entry for every other index; no entry
    /// is keyed. A raw 63,0,63 palette entry draws as its colour.
    #[test]
    fn a_raw_magenta_palette_entry_draws_opaque() {
        let shp = ShpFile::from_bytes(&make_test_shp_raw()).expect("parse");
        let mut pal_data: Vec<u8> = vec![0u8; 768];
        pal_data[6..9].copy_from_slice(&[63, 0, 63]); // index 2, pixel 1
        let palette = Palette::from_bytes(&pal_data).expect("palette");

        let rgba = shp.frame_to_rgba(0, &palette).expect("convert");

        assert_eq!(&rgba[4..8], &[252, 0, 252, 255]);
        assert_eq!(rgba[15], 0, "index 0 stays transparent");
    }
}
