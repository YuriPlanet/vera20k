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

// Outside data reaches this module: it refuses bad input, never panics.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable,
        clippy::indexing_slicing
    )
)]

use crate::assets::error::AssetError;
use crate::assets::vxl_decode::read_null_string;
use crate::util::read_helpers::read_u32_le;

/// Minimum file size: 16 (name) + 4 (frames) + 4 (sections) = 24 bytes.
const HVA_MIN_SIZE: usize = 24;

/// Bytes per section name.
const SECTION_NAME_SIZE: usize = 16;

/// Floats per transform matrix (3×4 row-major).
const MATRIX_FLOATS: usize = 12;

/// Bytes per transform matrix.
const MATRIX_BYTES: usize = MATRIX_FLOATS * 4;

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
        // Bytes 0-15: filename (skipped, not meaningful for parsing).
        let (Some(frame_count), Some(section_count), Some(body)) = (
            read_u32_le(data, 16),
            read_u32_le(data, 20),
            data.get(HVA_MIN_SIZE..),
        ) else {
            return Err(AssetError::InvalidHvaFile {
                reason: format!(
                    "File too small: {} bytes (need at least {})",
                    data.len(),
                    HVA_MIN_SIZE
                ),
            });
        };

        if section_count == 0 {
            return Err(AssetError::InvalidHvaFile {
                reason: "Section count is zero".to_string(),
            });
        }

        // Section names start at offset 24; the matrices follow them. Every
        // size below is checked, so a count the file cannot hold is an error
        // on any target instead of a wrapped product.
        let Some((names, matrix_data)) = SECTION_NAME_SIZE
            .checked_mul(section_count as usize)
            .and_then(|names_size| body.split_at_checked(names_size))
        else {
            return Err(AssetError::InvalidHvaFile {
                reason: format!(
                    "File too small for {} section names: {} bytes",
                    section_count,
                    data.len()
                ),
            });
        };
        let section_names: Vec<String> = names
            .chunks_exact(SECTION_NAME_SIZE)
            .map(read_null_string)
            .collect();

        // Transform matrices: 12 f32 per section per frame.
        let (all_matrices, _) = matrix_data.as_chunks::<MATRIX_BYTES>();
        let Some(matrices) = (frame_count as usize)
            .checked_mul(section_count as usize)
            .and_then(|total_matrices| all_matrices.get(..total_matrices))
        else {
            return Err(AssetError::InvalidHvaFile {
                reason: format!(
                    "File too small for {} frames of {} matrices: {} bytes",
                    frame_count,
                    section_count,
                    data.len()
                ),
            });
        };
        let transforms: Vec<[f32; 12]> = matrices
            .iter()
            .map(|matrix| {
                let mut floats = [0.0f32; MATRIX_FLOATS];
                for (float, bytes) in floats.iter_mut().zip(matrix.as_chunks::<4>().0) {
                    *float = f32::from_le_bytes(*bytes);
                }
                floats
            })
            .collect();

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

/// A minimal HVA for tests, client tests included: 2 frames of one section
/// named `body`, identity then translated by (1, 2, 3).
#[cfg(any(test, feature = "test-support"))]
pub fn minimal_test_hva() -> Vec<u8> {
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

#[cfg(all(test, tests_assets))]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hva_basic() {
        let data: Vec<u8> = minimal_test_hva();
        let hva: HvaFile = HvaFile::from_bytes(&data).expect("Should parse");
        assert_eq!(hva.frame_count, 2);
        assert_eq!(hva.section_count, 1);
        assert_eq!(hva.section_names.len(), 1);
        assert_eq!(hva.section_names[0], "body");
        assert_eq!(hva.transforms.len(), 2);
    }

    #[test]
    fn test_get_transform() {
        let data: Vec<u8> = minimal_test_hva();
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
