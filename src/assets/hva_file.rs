//! Parser for RA2 .hva voxel animation files.
//!
//! HVA files store per-frame bone transformation matrices for VXL voxel models.
//! Each frame has one 3×4 row-major matrix per section (limb), encoding rotation,
//! scale, and translation. The section count and order must match the paired VXL file.
//!
//! ## File structure
//! - 16-byte filename (null-padded ASCII)
//! - Frame count (u32) + section count (u32)
//! - Section names (16 bytes × section_count)
//! - Transform matrices (12 f32 × section_count × frame_count)
//!
//! ## Dependency rules
//! - Part of assets/ — no dependencies on game modules.

use crate::assets::error::AssetError;
use crate::assets::vxl_decode::read_null_string;
use crate::util::read_helpers::{read_f32_le, read_u32_le};

/// Minimum file size: 16 (name) + 4 (frames) + 4 (sections) = 24 bytes.
const HVA_MIN_SIZE: usize = 24;

/// Bytes per section name.
const SECTION_NAME_SIZE: usize = 16;

/// Floats per transform matrix (3×4 row-major).
const MATRIX_FLOATS: usize = 12;

/// A parsed HVA animation file containing per-frame per-section transforms.
#[derive(Debug)]
pub struct HvaFile {
    /// Number of animation frames.
    pub frame_count: u32,
    /// Number of sections (must match paired VXL limb count).
    pub section_count: u32,
    /// Section names (same order as VXL limbs).
    pub section_names: Vec<String>,
    /// Transform matrices: flat array of [f32; 12], indexed by
    /// `[frame * section_count + section]`.
    /// Each matrix is 3×4 row-major: rows 0-2 are rotation/scale,
    /// the 4th column of each row is translation (indices 3, 7, 11).
    pub transforms: Vec<[f32; 12]>,
}

impl HvaFile {
    /// Parse an HVA file from raw bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, AssetError> {
        if data.len() < HVA_MIN_SIZE {
            return Err(AssetError::InvalidHvaFile {
                reason: format!(
                    "File too small: {} bytes (need at least {})",
                    data.len(),
                    HVA_MIN_SIZE
                ),
            });
        }

        // Bytes 0-15: filename (skipped, not meaningful for parsing).
        let frame_count: u32 = read_u32_le(data, 16);
        let section_count: u32 = read_u32_le(data, 20);

        if section_count == 0 {
            return Err(AssetError::InvalidHvaFile {
                reason: "Section count is zero".to_string(),
            });
        }

        // Section names start at offset 24.
        let names_end: usize = HVA_MIN_SIZE + SECTION_NAME_SIZE * section_count as usize;
        if data.len() < names_end {
            return Err(AssetError::InvalidHvaFile {
                reason: format!(
                    "File too small for section names: {} bytes (need {})",
                    data.len(),
                    names_end
                ),
            });
        }

        let mut section_names: Vec<String> = Vec::with_capacity(section_count as usize);
        for i in 0..section_count as usize {
            let off: usize = HVA_MIN_SIZE + i * SECTION_NAME_SIZE;
            let name: String = read_null_string(&data[off..off + SECTION_NAME_SIZE]);
            section_names.push(name);
        }

        // Transform matrices: 12 f32 per section per frame.
        let total_matrices: usize = frame_count as usize * section_count as usize;
        let matrix_data_size: usize = total_matrices * MATRIX_FLOATS * 4;
        let matrix_start: usize = names_end;

        if data.len() < matrix_start + matrix_data_size {
            return Err(AssetError::InvalidHvaFile {
                reason: format!(
                    "File too small for matrices: {} bytes (need {})",
                    data.len(),
                    matrix_start + matrix_data_size
                ),
            });
        }

        let mut transforms: Vec<[f32; 12]> = Vec::with_capacity(total_matrices);
        let mut pos: usize = matrix_start;
        for _ in 0..total_matrices {
            let mut matrix: [f32; 12] = [0.0; 12];
            for (k, slot) in matrix.iter_mut().enumerate() {
                *slot = read_f32_le(data, pos + k * 4);
            }
            transforms.push(matrix);
            pos += MATRIX_FLOATS * 4;
        }

        Ok(HvaFile {
            frame_count,
            section_count,
            section_names,
            transforms,
        })
    }

    /// Get the transform matrix for a given frame and section index.
    ///
    /// Returns None if frame or section is out of range.
    pub fn get_transform(&self, frame: u32, section: u32) -> Option<&[f32; 12]> {
        if frame >= self.frame_count || section >= self.section_count {
            return None;
        }
        let idx: usize = frame as usize * self.section_count as usize + section as usize;
        self.transforms.get(idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal HVA: 2 frames, 1 section, identity matrices.
    fn make_test_hva() -> Vec<u8> {
        let mut data: Vec<u8> = Vec::new();

        // Filename (16 bytes).
        data.extend_from_slice(b"test.hva\0\0\0\0\0\0\0\0");

        // Frame count = 2, section count = 1.
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());

        // Section name (16 bytes).
        data.extend_from_slice(b"body\0\0\0\0\0\0\0\0\0\0\0\0");

        // Frame 0: identity matrix (3×4 row-major).
        // Row 0: [1, 0, 0, 0]
        data.extend_from_slice(&1.0f32.to_le_bytes());
        data.extend_from_slice(&0.0f32.to_le_bytes());
        data.extend_from_slice(&0.0f32.to_le_bytes());
        data.extend_from_slice(&0.0f32.to_le_bytes());
        // Row 1: [0, 1, 0, 0]
        data.extend_from_slice(&0.0f32.to_le_bytes());
        data.extend_from_slice(&1.0f32.to_le_bytes());
        data.extend_from_slice(&0.0f32.to_le_bytes());
        data.extend_from_slice(&0.0f32.to_le_bytes());
        // Row 2: [0, 0, 1, 0]
        data.extend_from_slice(&0.0f32.to_le_bytes());
        data.extend_from_slice(&0.0f32.to_le_bytes());
        data.extend_from_slice(&1.0f32.to_le_bytes());
        data.extend_from_slice(&0.0f32.to_le_bytes());

        // Frame 1: translated by (1, 2, 3).
        // Row 0: [1, 0, 0, 1]
        data.extend_from_slice(&1.0f32.to_le_bytes());
        data.extend_from_slice(&0.0f32.to_le_bytes());
        data.extend_from_slice(&0.0f32.to_le_bytes());
        data.extend_from_slice(&1.0f32.to_le_bytes());
        // Row 1: [0, 1, 0, 2]
        data.extend_from_slice(&0.0f32.to_le_bytes());
        data.extend_from_slice(&1.0f32.to_le_bytes());
        data.extend_from_slice(&0.0f32.to_le_bytes());
        data.extend_from_slice(&2.0f32.to_le_bytes());
        // Row 2: [0, 0, 1, 3]
        data.extend_from_slice(&0.0f32.to_le_bytes());
        data.extend_from_slice(&0.0f32.to_le_bytes());
        data.extend_from_slice(&1.0f32.to_le_bytes());
        data.extend_from_slice(&3.0f32.to_le_bytes());

        data
    }

    #[test]
    fn test_parse_hva_basic() {
        let data: Vec<u8> = make_test_hva();
        let hva: HvaFile = HvaFile::from_bytes(&data).expect("Should parse");
        assert_eq!(hva.frame_count, 2);
        assert_eq!(hva.section_count, 1);
        assert_eq!(hva.section_names.len(), 1);
        assert_eq!(hva.section_names[0], "body");
        assert_eq!(hva.transforms.len(), 2);
    }

    #[test]
    fn test_get_transform() {
        let data: Vec<u8> = make_test_hva();
        let hva: HvaFile = HvaFile::from_bytes(&data).expect("Should parse");

        // Frame 0: identity (translation = 0,0,0 at indices 3,7,11).
        let m0: &[f32; 12] = hva.get_transform(0, 0).expect("frame 0");
        assert!((m0[0] - 1.0).abs() < f32::EPSILON);
        assert!((m0[3] - 0.0).abs() < f32::EPSILON);
        assert!((m0[7] - 0.0).abs() < f32::EPSILON);
        assert!((m0[11] - 0.0).abs() < f32::EPSILON);

        // Frame 1: translation = (1, 2, 3).
        let m1: &[f32; 12] = hva.get_transform(1, 0).expect("frame 1");
        assert!((m1[3] - 1.0).abs() < f32::EPSILON);
        assert!((m1[7] - 2.0).abs() < f32::EPSILON);
        assert!((m1[11] - 3.0).abs() < f32::EPSILON);

        // Out of range returns None.
        assert!(hva.get_transform(2, 0).is_none());
        assert!(hva.get_transform(0, 1).is_none());
    }
}
