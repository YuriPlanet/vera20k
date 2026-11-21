//! Internal decode helpers for VXL voxel model parsing.
//! Handles sparse column-based voxel span decoding.
//! Part of assets/ — no dependencies on game modules.

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

use crate::assets::error::AssetError;
use crate::assets::vxl_file::VxlVoxel;

/// Read a null-terminated ASCII string from a fixed-size byte slice.
pub(crate) fn read_null_string(bytes: &[u8]) -> String {
    let end: usize = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

/// Decode all voxels for a single limb from the body data.
///
/// Uses the column start/end offset tables to locate each (x,y) column,
/// then walks voxel spans along the Z axis. Each span contains a z_skip
/// (gap before voxels), a count, then count×2 bytes of color+normal pairs.
pub(crate) fn decode_limb_voxels(
    data: &[u8],
    body_start: usize,
    span_start_off: u32,
    span_end_off: u32,
    data_span_off: u32,
    size_x: u8,
    size_y: u8,
    size_z: u8,
) -> Result<Vec<VxlVoxel>, AssetError> {
    let col_count: usize = size_x as usize * size_y as usize;
    // One little-endian i32 per column, in a table at `offset` of the body.
    let column_table = |offset: u32| -> Option<&[[u8; 4]]> {
        let start = body_start.checked_add(offset as usize)?;
        data.get(start..)?.as_chunks::<4>().0.get(..col_count)
    };

    // Validate offset tables fit in file.
    let (Some(start_table), Some(_end_table), Some(data_base)) = (
        column_table(span_start_off),
        column_table(span_end_off),
        body_start.checked_add(data_span_off as usize),
    ) else {
        return Err(AssetError::InvalidVxlFile {
            reason: format!(
                "Column offset tables extend past file end (start={}, end={}, cols={}, file={})",
                span_start_off,
                span_end_off,
                col_count,
                data.len()
            ),
        });
    };

    let mut voxels: Vec<VxlVoxel> = Vec::new();

    for (col_idx, col_start) in start_table.iter().enumerate() {
        let x: u8 = (col_idx % size_x as usize) as u8;
        let y: u8 = (col_idx / size_x as usize) as u8;

        // Column start is a byte offset relative to data_base.
        let col_start: i32 = i32::from_le_bytes(*col_start);

        // -1 means empty column (no voxels).
        if col_start < 0 {
            continue;
        }

        decode_column(
            data,
            data_base,
            col_start as usize,
            x,
            y,
            size_z,
            &mut voxels,
        )?;
    }

    Ok(voxels)
}

/// Decode voxel spans for a single (x, y) column.
///
/// Walks span-by-span: z_skip bytes to advance Z, count voxels to read,
/// then a dup_count byte (unused validation). Continues until z >= size_z.
fn decode_column(
    data: &[u8],
    data_base: usize,
    col_offset: usize,
    x: u8,
    y: u8,
    size_z: u8,
    voxels: &mut Vec<VxlVoxel>,
) -> Result<(), AssetError> {
    // A column that starts past the file has no spans.
    let mut rest: &[u8] = data_base
        .checked_add(col_offset)
        .and_then(|pos| data.get(pos..))
        .unwrap_or_default();
    let mut z: u8 = 0;

    while z < size_z {
        // A span is its two header bytes and at least the closing count.
        let Some((&[z_skip, count], after_header)) =
            rest.split_first_chunk::<2>().filter(|_| rest.len() >= 3)
        else {
            break;
        };
        rest = after_header;

        z = z.saturating_add(z_skip);

        if count == 0 {
            // Empty span — just skip. Read dup_count byte.
            rest = rest.get(1..).unwrap_or_default();
            continue;
        }

        // Read `count` voxels (2 bytes each: color + normal).
        let voxel_bytes: usize = count as usize * 2;
        let Some((span, after_span)) = rest.split_at_checked(voxel_bytes) else {
            return Err(AssetError::InvalidVxlFile {
                reason: format!(
                    "Voxel span needs {} bytes at offset {}, but file is {} bytes",
                    voxel_bytes,
                    data.len() - rest.len(),
                    data.len()
                ),
            });
        };

        for (v, &[color_index, normal_index]) in (0..count).zip(span.as_chunks::<2>().0) {
            let vz: u8 = z.saturating_add(v);
            if vz < size_z {
                voxels.push(VxlVoxel {
                    x,
                    y,
                    z: vz,
                    color_index,
                    normal_index,
                });
            }
        }

        // Skip dup_count byte (validation/padding — unused by us).
        rest = after_span.get(1..).unwrap_or_default();

        z = z.saturating_add(count);
    }

    Ok(())
}
