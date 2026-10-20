//! Parser for RA2 .vpl (voxel palette lookup) files.
//!
//! VPL files contain pre-computed lighting lookup tables for voxel rendering.
//! Each "page" maps a palette color index to a shaded version, allowing
//! Blinn-Phong lighting to be applied via simple table lookups rather than
//! per-pixel math. The page number comes from the normal-based brightness
//! calculation; the color index comes from the voxel's palette color.
//!
//! ## File structure
//! - 16-byte header: firstRemap(u32) + lastRemap(u32) + numSections(u32) + unknown(u32)
//! - 768-byte internal palette (256 RGB triplets, usually unused)
//! - numSections × 256-byte lookup tables
//!
//! ## Dependency rules
//! - Part of assets/ — no dependencies on game modules.

use crate::assets::error::AssetError;
use crate::util::read_helpers::read_u32_le;

/// Minimum file size: 16-byte header + 768-byte palette.
const VPL_MIN_SIZE: usize = 16 + 768;

/// A parsed VPL lighting lookup file.
#[derive(Debug)]
pub struct VplFile {
    /// Index of the first remappable palette entry.
    pub first_remap: u32,
    /// Index of the last remappable palette entry.
    pub last_remap: u32,
    /// Number of lookup pages (brightness levels).
    pub num_sections: u32,
    /// Lookup tables: pages[page][color_index] → shaded palette index.
    pages: Vec<[u8; 256]>,
}

impl VplFile {
    /// Parse a VPL file from raw bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, AssetError> {
        if data.len() < VPL_MIN_SIZE {
            return Err(AssetError::ParseError {
                format: "VPL".to_string(),
                detail: format!(
                    "File too small: {} bytes (need at least {})",
                    data.len(),
                    VPL_MIN_SIZE
                ),
            });
        }

        let first_remap: u32 = read_u32_le(data, 0);
        let last_remap: u32 = read_u32_le(data, 4);
        let num_sections: u32 = read_u32_le(data, 8);
        // Byte 12..16: unknown field (skip).

        let pages_start: usize = 16 + 768; // after header + palette
        let needed: usize = pages_start + num_sections as usize * 256;
        if data.len() < needed {
            return Err(AssetError::ParseError {
                format: "VPL".to_string(),
                detail: format!(
                    "File too small for {} sections: {} bytes (need {})",
                    num_sections,
                    data.len(),
                    needed
                ),
            });
        }

        let mut pages: Vec<[u8; 256]> = Vec::with_capacity(num_sections as usize);
        for i in 0..num_sections as usize {
            let offset: usize = pages_start + i * 256;
            let mut page: [u8; 256] = [0u8; 256];
            page.copy_from_slice(&data[offset..offset + 256]);
            pages.push(page);
        }

        Ok(VplFile {
            first_remap,
            last_remap,
            num_sections,
            pages,
        })
    }

    /// Access the raw page data for GPU upload.
    pub fn pages_slice(&self) -> &[[u8; 256]] {
        &self.pages
    }

    /// Look up the shaded palette index for a given brightness page and color.
    ///
    /// Clamps the page to the valid range (0..num_sections-1) so bright voxels
    /// still get proper lighting instead of falling back to raw unlit color.
    pub fn get_palette_index(&self, page: u8, color: u8) -> u8 {
        if self.pages.is_empty() {
            return color;
        }
        let page_idx: usize = (page as usize).min(self.pages.len() - 1);
        self.pages[page_idx][color as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid VPL: 2 sections, trivial lookup tables.
    fn make_test_vpl() -> Vec<u8> {
        let mut data: Vec<u8> = Vec::new();
        // Header: firstRemap=16, lastRemap=31, numSections=2, unknown=0
        data.extend_from_slice(&16u32.to_le_bytes());
        data.extend_from_slice(&31u32.to_le_bytes());
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        // Internal palette (768 bytes of zeros).
        data.extend_from_slice(&[0u8; 768]);
        // Page 0: identity (color → color).
        let page0: Vec<u8> = (0..=255u8).collect();
        data.extend_from_slice(&page0);
        // Page 1: all map to index 42.
        data.extend_from_slice(&[42u8; 256]);
        data
    }

    #[test]
    fn test_lookup() {
        let vpl: VplFile = VplFile::from_bytes(&make_test_vpl()).expect("Should parse");
        // Page 0 is identity.
        assert_eq!(vpl.get_palette_index(0, 100), 100);
        // Page 1 maps everything to 42.
        assert_eq!(vpl.get_palette_index(1, 200), 42);
        // Out-of-range page clamps to last page (page 1 maps everything to 42).
        assert_eq!(vpl.get_palette_index(5, 77), 42);
    }
}
