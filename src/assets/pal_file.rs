//! Parser for RA2 .pal palette files, and the one owner of gamemd's palette
//! conversion.
//!
//! A .pal file is exactly 768 bytes: 256 entries of 3 bytes each (R, G, B).
//! Color values are in the VGA 6-bit range (0–63).
//!
//! ## One conversion
//! Every gamemd palette load stores each component as `raw << 2` (63 becomes
//! 252) and nothing else: Init_Game `0x0052BA60` (ANIM, PALETTE, UNITSNO,
//! CAMEO, MOUSEPAL, GRFXTXT, TEMPERAT), Init_Theater `0x005349C0` (the
//! theater palette and UNIT%s.PAL), Read_Theater_TileSets_INI (ISO%s.PAL, loop
//! at `0x005454DB`), the startup splash `0x005312A0` (GLSMD.PAL) and
//! ConvertClass__BuildFromPaletteFile `0x0072ADE0`, which loads the shell,
//! loading-screen, sidebar and score palettes.
//! `tools/sidebar_oracle/palette.json` executes that last loop for every byte
//! value. The draw tables built from the
//! result, ConvertClass (`0x0048E740`, rows `0x004BBB00`) and
//! LightConvertClass::UpdateColors (`0x00556090`), convert each entry by its
//! colour and key none. [`Palette::from_bytes`] is that conversion for every
//! caller, in-game and UI alike.
//!
//! ## House Color Remapping
//! Palette indices 16–31 (16 entries) are reserved for "house colors" — the player's
//! team color. When rendering a unit, these 16 indices get replaced with the owning
//! player's color scheme. This is how RA2 distinguishes player units visually.
//!
//! ## Index 0
//! Palettes carry no transparency: every entry is opaque. The ordinary SHP
//! draw skips index 0 by index, never by palette content: BlitTransXlat
//! `0x004912B0` and RLEBlitTransXlat `0x004960A0`, which
//! ConvertClass::Select_Blitter `0x00490B90` and Select_RLE_Blitter
//! `0x00490E50` pick for flags such as 0 and 0x400 (`ShpFile::frame_to_rgba`).
//! Other blitters differ: BlitPlainXlat `0x004924C0`, picked by flag 0x20 for
//! an uncompressed frame, draws index 0.
//!
//! ## Dependency rules
//! - Part of assets/ — no dependencies on game modules.

use crate::assets::error::AssetError;

/// Number of colors in an RA2 palette (always 256 — one per possible byte value).
const PALETTE_COLOR_COUNT: usize = 256;

/// Size of a .pal file in bytes: 256 colors * 3 bytes (R, G, B) each.
const PAL_FILE_SIZE: usize = PALETTE_COLOR_COUNT * 3;

/// First palette index reserved for house (player) colors.
const HOUSE_COLOR_START: usize = 16;

/// Number of palette indices used for house colors (16 through 31 inclusive).
const HOUSE_COLOR_COUNT: usize = 16;

/// A single RGBA color with 8-bit channels.
///
/// RGBA because GPU textures and frame buffers take 4 bytes per pixel. Palette
/// entries are always opaque; transparency is decided where a frame is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// Create a fully opaque color.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
}

/// A 256-color palette in gamemd's converted 8-bit form.
///
/// Every entry is opaque (see the module notes on index 0). Indices 16–31 are
/// house colors that can be remapped per player.
#[derive(Debug, Clone)]
pub struct Palette {
    colors: [Color; PALETTE_COLOR_COUNT],
}

impl Palette {
    /// Convert a .pal file the way every gamemd palette load does (module
    /// notes): each 6-bit component becomes `raw << 2`.
    ///
    /// Input must be exactly 768 bytes (256 colors * 3 bytes RGB).
    pub fn from_bytes(data: &[u8]) -> Result<Self, AssetError> {
        if data.len() != PAL_FILE_SIZE {
            return Err(AssetError::InvalidPalSize {
                expected: PAL_FILE_SIZE,
                actual: data.len(),
            });
        }
        Ok(Self::from_rgb(|index| {
            let base = usize::from(index) * 3;
            [data[base] << 2, data[base + 1] << 2, data[base + 2] << 2]
        }))
    }

    /// A palette of 8-bit colours a caller computed itself instead of reading a
    /// file, such as Init_Theater's ramp for a missing theater palette.
    pub fn from_rgb(mut rgb: impl FnMut(u8) -> [u8; 3]) -> Self {
        Self {
            colors: std::array::from_fn(|index| {
                let [r, g, b] = rgb(index as u8);
                Color::rgb(r, g, b)
            }),
        }
    }

    /// The color of palette entry `index`.
    pub fn color(&self, index: u8) -> Color {
        self.colors[usize::from(index)]
    }

    /// All 256 entries in index order.
    pub fn colors(&self) -> &[Color; PALETTE_COLOR_COUNT] {
        &self.colors
    }

    /// Create a copy of this palette with house colors (indices 16–31) replaced.
    ///
    /// Each player has a unique set of 16 colors. When rendering a unit owned by
    /// that player, we swap palette indices 16–31 with the player's house colors.
    /// This is how RA2 makes allied units blue, soviet units red, etc.
    pub fn with_house_colors(&self, house_colors: &[Color; HOUSE_COLOR_COUNT]) -> Palette {
        let mut remapped: Palette = self.clone();
        for (slot, color) in remapped.colors
            [HOUSE_COLOR_START..HOUSE_COLOR_START + HOUSE_COLOR_COUNT]
            .iter_mut()
            .zip(house_colors)
        {
            *slot = Color::rgb(color.r, color.g, color.b);
        }
        remapped
    }

    /// Convert this palette's colors to a flat RGBA byte array (1024 bytes).
    ///
    /// Useful for uploading the palette as a 256x1 GPU texture.
    pub fn to_rgba_bytes(&self) -> Vec<u8> {
        let mut bytes: Vec<u8> = Vec::with_capacity(PALETTE_COLOR_COUNT * 4);
        for color in &self.colors {
            bytes.push(color.r);
            bytes.push(color.g);
            bytes.push(color.b);
            bytes.push(color.a);
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gsi_02_13_palette_uses_native_component_shift() {
        let mut data: Vec<u8> = vec![0u8; PAL_FILE_SIZE];
        data[3] = 63;
        data[4] = 31;
        data[5] = 1;

        let pal: Palette = Palette::from_bytes(&data).expect("Should parse valid palette");

        assert_eq!(pal.color(1), Color::rgb(252, 124, 4));
    }

    /// Every gamemd palette load stores `raw << 2` and nothing else: Init_Game
    /// `0x0052BA60` (ANIM, PALETTE, UNITSNO, CAMEO, MOUSEPAL, GRFXTXT),
    /// Init_Theater `0x005349C0`, Read_Theater_TileSets_INI (`0x005454DB`) and
    /// ConvertClass__BuildFromPaletteFile `0x0072ADE0`. ConvertClass
    /// (`0x0048E740` -> `0x004BBB00`) then packs all 256 entries. No entry is
    /// keyed: a raw 63,0,63 entry, index 0 included, stays its colour.
    #[test]
    fn every_entry_decodes_opaque_including_raw_magenta() {
        let mut data: Vec<u8> = vec![0u8; PAL_FILE_SIZE];
        data[..3].copy_from_slice(&[63, 0, 63]);
        data[204 * 3..204 * 3 + 3].copy_from_slice(&[63, 0, 63]);

        let rgba = Palette::from_bytes(&data).expect("parse").to_rgba_bytes();

        assert_eq!(&rgba[..4], &[252, 0, 252, 255]);
        assert_eq!(&rgba[204 * 4..205 * 4], &[252, 0, 252, 255]);
        assert!(rgba.chunks_exact(4).all(|entry| entry[3] == 255));
    }

    #[test]
    fn test_reject_wrong_size() {
        let data: Vec<u8> = vec![0u8; 100]; // Too small
        let result = Palette::from_bytes(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_to_rgba_bytes_length() {
        let data: Vec<u8> = vec![0u8; PAL_FILE_SIZE];
        let pal: Palette = Palette::from_bytes(&data).expect("Should parse");
        let rgba: Vec<u8> = pal.to_rgba_bytes();
        // 256 colors * 4 bytes (RGBA) each = 1024 bytes
        assert_eq!(rgba.len(), 1024);
    }

    #[test]
    fn test_house_color_remap() {
        let data: Vec<u8> = vec![0u8; PAL_FILE_SIZE];
        let pal: Palette = Palette::from_bytes(&data).expect("Should parse");

        // Create red house colors.
        let red_house: [Color; HOUSE_COLOR_COUNT] = [Color::rgb(255, 0, 0); HOUSE_COLOR_COUNT];
        let remapped: Palette = pal.with_house_colors(&red_house);

        // Indices 16–31 should now be red.
        for i in HOUSE_COLOR_START..(HOUSE_COLOR_START + HOUSE_COLOR_COUNT) {
            assert_eq!(remapped.colors()[i], Color::rgb(255, 0, 0));
        }

        // Other indices should be unchanged (still black from the zero input).
        assert_eq!(remapped.color(0), Color::rgb(0, 0, 0));
        assert_eq!(remapped.color(32), Color::rgb(0, 0, 0));
    }
}
