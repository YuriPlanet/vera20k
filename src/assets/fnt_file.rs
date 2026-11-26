//! Parser for RA2/YR `.fnt` bitmap font files (the "fonT" variant).
//!
//! Binary layout:
//!   - 4 bytes: magic `0x546E6F66` ("fonT")
//!   - 6 × u32: header fields
//!   - 128 KB: glyph lookup table (65536 × u16, codepoint → 1-based glyph index)
//!   - N bytes: glyph bitmap data (field[4] × field[5] bytes)
//!
//! Each glyph slot is `glyph_stride` bytes:
//!   - byte 0: glyph pixel width
//!   - remaining: 1-bit-per-pixel bitmap rows, MSB = leftmost pixel
//!
//! See docs/SIDEBAR_READY_TEXT_RENDERING.md for format details.

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

use std::collections::HashMap;

use crate::util::read_helpers::read_u32_le;

/// Magic value for the "fonT" variant used by GAME.FNT.
const FONT_MAGIC: u32 = 0x546E_6F66;
/// A glyph slot that at least this many code points share is a placeholder,
/// not a drawing of each of them. Retail GAME.FNT has four: it points 3,897
/// code points (1,937 of GB2312's 6,763 simplified hanzi among them) at one
/// empty box, slot 345, and 467 more, most of Latin Extended-B, IPA and
/// general punctuation, at two blanks and a mark (slots 229, 561 and 622). No
/// real glyph is shared that widely.
const PLACEHOLDER_SHARE: u32 = 64;
/// Size of the glyph lookup table in bytes (65536 entries × 2 bytes).
const LOOKUP_TABLE_BYTES: usize = 65536 * 2;
/// Header size in bytes (magic + 6 fields).
const HEADER_BYTES: usize = 4 + 6 * 4;
/// VERA's bounds on a font file, which decides how much memory its glyphs
/// take: a glyph's width is one byte, so 32 bytes hold its widest row, and
/// retail GAME.FNT has 16 rows of 3 bytes. A file past them is refused like
/// any other malformed font. gamemd's reader is not ported; these limits are
/// VERA's own.
const MAX_BYTES_PER_ROW: u32 = 32;
const MAX_BITMAP_ROWS: u32 = 255;
/// The most decoded glyph pixels (RGBA bytes) one font may ask for. Code
/// points can share a slot, so a small file could otherwise name one large
/// glyph 65,536 times. Retail GAME.FNT decodes to under 64 MiB.
const MAX_DECODED_BYTES: usize = 256 << 20;

/// Immutable identity of the exact bytes accepted by the FNT parser.
/// Retained by the atlas owner so capture can prove which font it consumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FntSourceIdentity {
    pub byte_length: u64,
    pub sha256: String,
}

/// Parsed bitmap font from a `.fnt` file.
pub struct FntFile {
    source_identity: Option<FntSourceIdentity>,
    /// Cell height for layout purposes (includes 1px line gap).
    pub cell_height: u32,
    /// Number of bitmap scanlines per glyph (cell_height - 1 typically).
    pub bitmap_rows: u32,
    /// Bytes per bitmap row per glyph (each byte = 8 pixels).
    pub bytes_per_row: u32,
    /// Bytes per glyph slot in the data (1 + bytes_per_row × bitmap_rows).
    pub glyph_stride: u32,
    /// Per-glyph data keyed by Unicode codepoint.
    glyphs: HashMap<u16, FntGlyph>,
    /// The slots that [`PLACEHOLDER_SHARE`] or more code points share.
    placeholder_slots: Vec<u32>,
}

/// A single decoded glyph.
pub struct FntGlyph {
    /// The 1-based glyph slot the lookup table names for this code point; 0
    /// for a glyph drawn in GAME.FNT's style that is not its own.
    pub slot: u32,
    /// Width in pixels (variable per character).
    pub width: u32,
    /// RGBA bitmap: `width × bitmap_rows × 4` bytes.
    /// White (255,255,255,255) where bits are set, transparent elsewhere.
    pub rgba: Vec<u8>,
}

impl FntFile {
    /// Parse a `.fnt` file from raw bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, crate::assets::error::AssetError> {
        let malformed = |detail: String| crate::assets::error::AssetError::ParseError {
            format: "FNT".into(),
            detail,
        };
        let min_len = HEADER_BYTES + LOOKUP_TABLE_BYTES;
        let too_small = || malformed(format!("file too small: {} < {}", data.len(), min_len));
        let word = |index: usize| read_u32_le(data, index * 4);
        // Word 1 is unused; the comments give retail GAME.FNT's values.
        let (
            Some(magic),
            Some(bytes_per_row),   // 3
            Some(bitmap_rows),     // 16
            Some(cell_height),     // 17
            Some(num_glyph_slots), // 29655
            Some(glyph_stride),    // 49
        ) = (word(0), word(2), word(3), word(4), word(5), word(6))
        else {
            return Err(too_small());
        };
        let Some((lookup, bitmap_data)) = data
            .get(HEADER_BYTES..)
            .and_then(|rest| rest.split_at_checked(LOOKUP_TABLE_BYTES))
        else {
            return Err(too_small());
        };

        if magic != FONT_MAGIC {
            return Err(malformed(format!(
                "bad magic: 0x{magic:08X}, expected 0x{FONT_MAGIC:08X}"
            )));
        }

        // Sanity checks.
        if bytes_per_row == 0 || bitmap_rows == 0 || glyph_stride == 0 {
            return Err(malformed("zero-valued header field".into()));
        }
        if bytes_per_row > MAX_BYTES_PER_ROW || bitmap_rows > MAX_BITMAP_ROWS {
            return Err(malformed(format!(
                "glyph bitmap of {bitmap_rows} rows of {bytes_per_row} bytes is past \
                 {MAX_BITMAP_ROWS} rows of {MAX_BYTES_PER_ROW}"
            )));
        }
        let expected_stride = 1 + bytes_per_row * bitmap_rows;
        if glyph_stride < expected_stride {
            return Err(malformed(format!(
                "glyph_stride {glyph_stride} holds less than its {bitmap_rows} rows of \
                 {bytes_per_row} bytes ({expected_stride})"
            )));
        }
        if glyph_stride != expected_stride {
            log::warn!(
                "FNT glyph_stride {} != expected {} (1 + {} × {}), using file value",
                glyph_stride,
                expected_stride,
                bytes_per_row,
                bitmap_rows,
            );
        }

        let glyph_stride_bytes = glyph_stride as usize;
        let bitmap_data_size = (num_glyph_slots as usize).checked_mul(glyph_stride_bytes);
        if bitmap_data_size.is_none_or(|size| bitmap_data.len() < size) {
            return Err(malformed(format!(
                "file too small for {num_glyph_slots} glyphs of {glyph_stride} bytes: {}",
                data.len()
            )));
        }

        let mut glyphs = HashMap::new();
        let mut slot_uses: HashMap<u32, u32> = HashMap::new();
        let mut decoded_bytes: usize = 0;
        for (codepoint, entry) in (0u16..=u16::MAX).zip(lookup.chunks_exact(2)) {
            let index = u32::from(u16::from_le_bytes([entry[0], entry[1]]));
            if index == 0 {
                continue;
            }
            let Some(glyph_bytes) = glyph_stride_bytes
                .checked_mul((index - 1) as usize)
                .and_then(|glyph_off| bitmap_data.get(glyph_off..)?.get(..glyph_stride_bytes))
            else {
                continue;
            };
            let Some((&width, bitmap)) = glyph_bytes.split_first() else {
                continue;
            };
            let width = u32::from(width);
            if width == 0 {
                continue;
            }

            let rgba = decode_glyph_bitmap(bitmap, width, bitmap_rows, bytes_per_row);
            decoded_bytes += rgba.len();
            if decoded_bytes > MAX_DECODED_BYTES {
                return Err(malformed(format!(
                    "glyphs decode to more than {MAX_DECODED_BYTES} bytes"
                )));
            }
            *slot_uses.entry(index).or_default() += 1;
            glyphs.insert(
                codepoint,
                FntGlyph {
                    slot: index,
                    width,
                    rgba,
                },
            );
        }
        let placeholder_slots = slot_uses
            .into_iter()
            .filter(|&(_, uses)| uses >= PLACEHOLDER_SHARE)
            .map(|(slot, _)| slot)
            .collect();

        log::info!(
            "Parsed GAME.FNT: {} glyphs, cell_height={}, bitmap_rows={}, bytes_per_row={}",
            glyphs.len(),
            cell_height,
            bitmap_rows,
            bytes_per_row,
        );

        Ok(Self {
            source_identity: Some(FntSourceIdentity {
                byte_length: data.len() as u64,
                sha256: crate::util::sha256::sha256_hex(data),
            }),
            cell_height,
            bitmap_rows,
            bytes_per_row,
            glyph_stride,
            glyphs,
            placeholder_slots,
        })
    }

    /// A font of GAME.FNT's `cell_height` and `bitmap_rows` that draws
    /// nothing, for interface text before, or without, the game's files: the
    /// interface then draws every letter in GAME.FNT's style instead.
    pub fn blank(cell_height: u32, bitmap_rows: u32) -> Self {
        Self {
            source_identity: None,
            cell_height,
            bitmap_rows,
            bytes_per_row: 3,
            glyph_stride: 1 + 3 * bitmap_rows,
            glyphs: HashMap::new(),
            placeholder_slots: Vec::new(),
        }
    }

    /// Every codepoint as one solid `width`-pixel, one-row glyph, so tests can
    /// place text without retail GAME.FNT.
    #[cfg(any(test, feature = "test-support"))]
    pub fn solid_for_test(width: u32) -> Self {
        let rgba = [255_u8; 4].repeat(width as usize);
        Self {
            source_identity: None,
            cell_height: 2,
            bitmap_rows: 1,
            bytes_per_row: 1,
            glyph_stride: 2,
            glyphs: (0..=u16::MAX)
                .map(|codepoint| {
                    let rgba = rgba.clone();
                    let slot = 1;
                    (codepoint, FntGlyph { slot, width, rgba })
                })
                .collect(),
            placeholder_slots: Vec::new(),
        }
    }

    pub fn source_identity(&self) -> Option<&FntSourceIdentity> {
        self.source_identity.as_ref()
    }

    /// Look up a glyph by Unicode codepoint.
    pub fn glyph(&self, codepoint: u16) -> Option<&FntGlyph> {
        self.glyphs.get(&codepoint)
    }

    /// Whether `codepoint` has a glyph of its own: not missing, not a shared
    /// placeholder (see [`PLACEHOLDER_SHARE`]).
    pub fn draws(&self, codepoint: u16) -> bool {
        self.glyphs
            .get(&codepoint)
            .is_some_and(|glyph| !self.placeholder_slots.contains(&glyph.slot))
    }

    /// Every code point with a glyph of its own ([`Self::draws`]).
    pub fn drawn_codepoints(&self) -> impl Iterator<Item = (u16, &FntGlyph)> {
        self.glyphs
            .iter()
            .filter(|(_, glyph)| !self.placeholder_slots.contains(&glyph.slot))
            .map(|(&codepoint, glyph)| (codepoint, glyph))
    }

    /// Measure the pixel width of a text string.
    /// Uses char_advance = glyph_width + 1 (1px inter-character spacing).
    pub fn text_width(&self, text: &str) -> u32 {
        let mut width: u32 = 0;
        let mut count: u32 = 0;
        for ch in text.chars() {
            let cp = ch as u32;
            if cp > u16::MAX as u32 {
                continue;
            }
            if let Some(g) = self.glyphs.get(&(cp as u16)) {
                width += g.width;
                count += 1;
            }
        }
        // Inter-char spacing of 1px between each pair of glyphs.
        if count > 1 {
            width += count - 1;
        }
        width
    }
}

/// Decode 1-bit-per-pixel glyph bitmap rows into RGBA.
/// Each byte stores 8 pixels, MSB = leftmost pixel. A pixel whose byte lies
/// past the glyph's data is transparent.
/// Output: width × bitmap_rows × 4 bytes (white on transparent).
fn decode_glyph_bitmap(bitmap: &[u8], width: u32, bitmap_rows: u32, bytes_per_row: u32) -> Vec<u8> {
    let (width, rows, bytes_per_row) =
        (width as usize, bitmap_rows as usize, bytes_per_row as usize);
    let mut rgba = vec![0u8; width * rows * 4];
    if width == 0 {
        return rgba;
    }
    for (row, out_row) in rgba.chunks_exact_mut(width * 4).enumerate() {
        let row_start = row * bytes_per_row;
        for (px, out) in out_row.chunks_exact_mut(4).enumerate() {
            let bit_idx = 7 - (px % 8); // MSB first
            let set = bitmap
                .get(row_start + px / 8)
                .is_some_and(|byte| (byte >> bit_idx) & 1 != 0);
            if set {
                out.fill(255);
            }
        }
    }
    rgba
}

#[cfg(all(test, tests_assets))]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn parse_real_game_fnt() {
        use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
        use std::path::Path;
        let dir = std::env::var("RA2_DIR").expect("Set RA2_DIR to your RA2/YR install directory");
        let ra2_dir = Path::new(&dir);
        let mut mgr =
            AssetManager::new(ra2_dir, MediaArchiveMode::STOCK_DIGITAL).expect("AssetManager");
        mgr.load_all_disk_mixes().ok();
        for name in [
            "local.mix",
            "localmd.mix",
            "cache.mix",
            "cachemd.mix",
            "conquer.mix",
            "conquermd.mix",
        ] {
            mgr.load_nested(name).ok();
        }
        let data = mgr.get("GAME.FNT").expect("GAME.FNT not found");
        let fnt = FntFile::from_bytes(&data).expect("parse failed");

        assert_eq!(fnt.cell_height, 17);
        assert_eq!(fnt.bitmap_rows, 16);
        assert_eq!(fnt.bytes_per_row, 3);
        assert_eq!(fnt.glyph_stride, 49);

        // 'R' (0x52) should exist and have a reasonable width.
        let r = fnt.glyph(b'R' as u16).expect("glyph 'R' missing");
        assert!(r.width >= 3 && r.width <= 20, "R width = {}", r.width);
        assert_eq!(
            r.rgba.len(),
            (r.width * fnt.bitmap_rows * 4) as usize,
            "RGBA size mismatch"
        );

        // Measure "Ready" — should be a reasonable width.
        let w = fnt.text_width("Ready");
        assert!((15..=60).contains(&w), "Ready width = {}", w);
    }

    #[test]
    fn parsed_source_identity_is_retained_after_the_input_buffer_changes() {
        let mut data = vec![0_u8; HEADER_BYTES + LOOKUP_TABLE_BYTES + 2];
        for (index, word) in [FONT_MAGIC, 20, 1, 1, 2, 1, 2].into_iter().enumerate() {
            data[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        data[HEADER_BYTES + usize::from(b'A') * 2] = 1;
        data[HEADER_BYTES + LOOKUP_TABLE_BYTES..].copy_from_slice(&[1, 0x80]);
        let fnt = FntFile::from_bytes(&data).expect("synthetic FNT");
        let source = fnt.source_identity().expect("parsed source identity");
        assert_eq!(source.byte_length, 131_102);
        // Python hashlib over this synthetic input, not a native parity golden.
        assert_eq!(
            source.sha256,
            "f5bae7456e7253a70ae4b8fda31a298b47f371f9c5a40daaeb570ec4fd47d002"
        );
        *data.last_mut().unwrap() = 0;
        let changed = FntFile::from_bytes(&data).expect("changed FNT");
        assert_ne!(changed.source_identity(), fnt.source_identity());
        assert_eq!(
            source.sha256,
            "f5bae7456e7253a70ae4b8fda31a298b47f371f9c5a40daaeb570ec4fd47d002"
        );
    }

    /// A header whose rows the glyph data cannot hold, or whose bitmap is past
    /// VERA's bounds, is refused before any glyph is decoded: a 131 KB file
    /// asking for four million rows used to allocate gigabytes per glyph, and
    /// one with a short stride was read as if its rows were there.
    #[test]
    fn header_rows_the_glyph_data_cannot_hold_are_refused() {
        let font = |bytes_per_row: u32, bitmap_rows: u32, glyph_stride: u32| {
            let mut data = vec![0_u8; HEADER_BYTES + LOOKUP_TABLE_BYTES + 2];
            let words = [
                FONT_MAGIC,
                20,
                bytes_per_row,
                bitmap_rows,
                2,
                1,
                glyph_stride,
            ];
            for (index, word) in words.into_iter().enumerate() {
                data[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
            }
            data[HEADER_BYTES + usize::from(b'A') * 2] = 1;
            data[HEADER_BYTES + LOOKUP_TABLE_BYTES..].copy_from_slice(&[255, 0x80]);
            FntFile::from_bytes(&data)
        };
        assert!(font(1, 1, 2).is_ok());
        assert!(font(1, 16, 2).is_err(), "sixteen rows in a one-byte glyph");
        assert!(
            font(1, 4_000_000, 4_000_001).is_err(),
            "rows past the bound"
        );
    }

    /// Every slot shared by [`PLACEHOLDER_SHARE`] or more code points is a
    /// placeholder, not only the most shared one.
    #[test]
    fn every_widely_shared_slot_is_a_placeholder() {
        let mut data = vec![0_u8; HEADER_BYTES + LOOKUP_TABLE_BYTES + 3 * 2];
        for (index, word) in [FONT_MAGIC, 20, 1, 1, 2, 3, 2].into_iter().enumerate() {
            data[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        let shares = [(0x100, 70, 1), (0x200, PLACEHOLDER_SHARE as usize, 2)];
        for (first, count, slot) in shares {
            for codepoint in first..first + count {
                data[HEADER_BYTES + codepoint * 2] = slot;
            }
        }
        data[HEADER_BYTES + usize::from(b'A') * 2] = 3;
        data[HEADER_BYTES + LOOKUP_TABLE_BYTES..].copy_from_slice(&[1, 0x80, 1, 0x80, 1, 0x80]);
        let fnt = FntFile::from_bytes(&data).expect("synthetic FNT");
        let drawn: Vec<u16> = fnt.drawn_codepoints().map(|(cp, _)| cp).collect();
        assert_eq!(drawn, [u16::from(b'A')]);
        assert!(!fnt.draws(0x100) && !fnt.draws(0x200) && fnt.draws(u16::from(b'A')));
    }

    #[test]
    fn decode_glyph_bitmap_basic() {
        // 2 pixels wide, 2 rows, 1 byte per row.
        // Row 0: 0b11000000 → pixels 0,1 set
        // Row 1: 0b01000000 → pixel 1 set
        let bitmap = [0b1100_0000, 0b0100_0000];
        let rgba = decode_glyph_bitmap(&bitmap, 2, 2, 1);
        assert_eq!(rgba.len(), 2 * 2 * 4);
        // Row 0: px0 = white, px1 = white
        assert_eq!(rgba[0..4], [255, 255, 255, 255]);
        assert_eq!(rgba[4..8], [255, 255, 255, 255]);
        // Row 1: px0 = transparent, px1 = white
        assert_eq!(rgba[8..12], [0, 0, 0, 0]);
        assert_eq!(rgba[12..16], [255, 255, 255, 255]);
    }
}
