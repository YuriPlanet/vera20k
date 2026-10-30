//! Internal decode helpers for TMP terrain tile parsing.
//! Handles per-tile cell parsing, diamond pixel unpacking, and extra data overlay.
//! Part of assets/ — no dependencies on game modules.

use crate::assets::error::AssetError;
use crate::assets::tmp_file::TmpTile;
use crate::util::read_helpers::{read_i32_le, read_u32_le};

/// Size of the per-tile cell header in bytes (before pixel data).
const TILE_HEADER_SIZE: usize = 52;

/// Decode budget for one tile's pixel plane. Retail and custom terrain tiles
/// measure in kilobytes; anything larger is a malformed header whose
/// dimensions must not steer the allocation (issue: asset header counts and
/// dimensions trigger oversized allocations). The diamond accumulator checks
/// the same budget, which also bounds its per-row loop on absurd heights.
const MAX_DECODED_TILE_BYTES: usize = 64 * 1024 * 1024;

/// Bit flag: tile has extra pixel data (cliff faces, shadows).
const FLAG_HAS_EXTRA_DATA: u32 = 0x01;

/// Bit flag: tile has per-pixel Z-buffer data (depth for occlusion).
/// When NOT set, ZData bytes do not exist after diamond pixel data.
const FLAG_HAS_Z_DATA: u32 = 0x02;

/// Bit flag: tile variants are deterministic damaged states (bridges),
/// not randomly-selected visual diversity picks (normal terrain).
const FLAG_HAS_DAMAGED_DATA: u32 = 0x04;

/// Initial diamond row width in pixels. All TS/RA2 tiles start with 4 pixels.
pub(crate) const DIAMOND_INITIAL_WIDTH: u32 = 4;

/// Diamond row width increment/decrement per row.
pub(crate) const DIAMOND_WIDTH_STEP: u32 = 4;

/// Parse a single tile cell at the given file offset.
///
/// Reads the 52-byte header, unpacks diamond pixel and depth data,
/// and handles optional extra data (cliff faces, shadows).
pub(crate) fn parse_tile_cell(
    data: &[u8],
    offset: usize,
    tile_width: u32,
    tile_height: u32,
) -> Result<TmpTile, AssetError> {
    let header_end: usize = offset
        .checked_add(TILE_HEADER_SIZE)
        .ok_or_else(|| invalid_tmp(format!("Tile cell offset {} overflows", offset)))?;
    if header_end > data.len() {
        return Err(AssetError::InvalidTmpFile {
            reason: format!(
                "Tile cell at offset {} extends past file end ({})",
                offset,
                data.len()
            ),
        });
    }

    let stored_x: i32 = read_i32_le(data, offset);
    let stored_y: i32 = read_i32_le(data, offset + 4);
    let extra_data_offset: u32 = read_u32_le(data, offset + 8);
    let z_data_offset: u32 = read_u32_le(data, offset + 12);
    let extra_z_data_offset: u32 = read_u32_le(data, offset + 16);
    let raw_extra_x: i32 = read_i32_le(data, offset + 20);
    let raw_extra_y: i32 = read_i32_le(data, offset + 24);
    let extra_width: u32 = read_u32_le(data, offset + 28);
    let extra_height: u32 = read_u32_le(data, offset + 32);
    let flags: u32 = read_u32_le(data, offset + 36);

    let height: u8 = data[offset + 40];
    let terrain_type: u8 = data[offset + 41];
    let ramp_type: u8 = data[offset + 42];
    let radar_left: [u8; 3] = [data[offset + 43], data[offset + 44], data[offset + 45]];
    let radar_right: [u8; 3] = [data[offset + 46], data[offset + 47], data[offset + 48]];

    let has_extra: bool = (flags & FLAG_HAS_EXTRA_DATA) != 0;
    let has_z_data: bool = (flags & FLAG_HAS_Z_DATA) != 0;
    let has_damaged_data: bool = (flags & FLAG_HAS_DAMAGED_DATA) != 0;
    let (extra_x, extra_y): (i32, i32) = if has_extra {
        (
            raw_extra_x.checked_sub(stored_x).ok_or_else(|| {
                invalid_tmp(format!(
                    "Extra X origin {} cannot be represented relative to stored tile X {}",
                    raw_extra_x, stored_x
                ))
            })?,
            raw_extra_y.checked_sub(stored_y).ok_or_else(|| {
                invalid_tmp(format!(
                    "Extra Y origin {} cannot be represented relative to stored tile Y {}",
                    raw_extra_y, stored_y
                ))
            })?,
        )
    } else {
        (0, 0)
    };

    // Compute bounding rectangle encompassing diamond + any extra data.
    let (pixel_width, pixel_height, offset_x, offset_y) = if has_extra {
        let min_x: i64 = 0i64.min(i64::from(extra_x));
        let min_y: i64 = 0i64.min(i64::from(extra_y));
        let max_x: i64 = i64::from(tile_width).max(i64::from(extra_x) + i64::from(extra_width));
        let max_y: i64 = i64::from(tile_height).max(i64::from(extra_y) + i64::from(extra_height));
        let pixel_width: u32 = u32::try_from(max_x - min_x).map_err(|_| {
            invalid_tmp(format!(
                "Tile plus extra X bounds [{}, {}) do not fit a decoded image",
                min_x, max_x
            ))
        })?;
        let pixel_height: u32 = u32::try_from(max_y - min_y).map_err(|_| {
            invalid_tmp(format!(
                "Tile plus extra Y bounds [{}, {}) do not fit a decoded image",
                min_y, max_y
            ))
        })?;
        (
            pixel_width,
            pixel_height,
            i32::try_from(min_x)
                .map_err(|_| invalid_tmp(format!("Decoded X origin {} does not fit i32", min_x)))?,
            i32::try_from(min_y)
                .map_err(|_| invalid_tmp(format!("Decoded Y origin {} does not fit i32", min_y)))?,
        )
    } else {
        (tile_width, tile_height, 0, 0)
    };

    // Validate every payload slice before reserving the decoded planes: a
    // malformed header's dimensions and offsets must prove the bytes they
    // promise exist in the file before any allocation reflects them.
    let diamond_bytes: usize = diamond_byte_count(tile_width, tile_height)?;
    let diamond_data: &[u8] = checked_cell_slice(
        data,
        offset,
        TILE_HEADER_SIZE as u32,
        diamond_bytes,
        "diamond color",
    )?;
    let z_data: Option<&[u8]> = if has_z_data {
        Some(checked_cell_slice(
            data,
            offset,
            z_data_offset,
            diamond_bytes,
            "diamond depth",
        )?)
    } else {
        None
    };
    let extra_pixel_count: usize = if has_extra {
        usize::try_from(extra_width)
            .ok()
            .and_then(|width| {
                usize::try_from(extra_height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .ok_or_else(|| {
                invalid_tmp(format!(
                    "Extra dimensions {}x{} overflow their plane size",
                    extra_width, extra_height
                ))
            })?
    } else {
        0
    };
    let extra_data: &[u8] = if has_extra {
        checked_cell_slice(
            data,
            offset,
            extra_data_offset,
            extra_pixel_count,
            "extra color",
        )?
    } else {
        &[]
    };
    let extra_z_data: Option<&[u8]> = if has_extra && has_z_data {
        Some(checked_cell_slice(
            data,
            offset,
            extra_z_data_offset,
            extra_pixel_count,
            "extra depth",
        )?)
    } else {
        None
    };

    let buf_size: usize = usize::try_from(pixel_width)
        .ok()
        .and_then(|width| {
            usize::try_from(pixel_height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or_else(|| {
            invalid_tmp(format!(
                "Decoded tile dimensions {}x{} overflow the pixel buffer",
                pixel_width, pixel_height
            ))
        })?;
    if buf_size > MAX_DECODED_TILE_BYTES {
        return Err(invalid_tmp(format!(
            "Decoded tile dimensions {}x{} exceed the {}-byte tile budget",
            pixel_width, pixel_height, MAX_DECODED_TILE_BYTES
        )));
    }
    let mut pixels: Vec<u8> = zeroed_plane(buf_size, "color")?;
    let mut depth: Vec<u8> = zeroed_plane(buf_size, "depth")?;

    // The color diamond is the only fixed section: it starts immediately after
    // the cell header. All optional planes use their stored cell-relative offsets.
    unpack_diamond(
        diamond_data,
        tile_width,
        tile_height,
        &mut pixels,
        pixel_width,
        offset_x,
        offset_y,
    )?;

    // ZData (per-pixel depth) only exists when HasZData flag (bit 1) is set.
    // Without this check, we'd consume bytes belonging to ExtraData, corrupting
    // cliff face graphics. See ra2_yr_map_terrain.md §1.4.
    if let Some(z_data) = z_data {
        unpack_diamond(
            z_data,
            tile_width,
            tile_height,
            &mut depth,
            pixel_width,
            offset_x,
            offset_y,
        )?;
    }

    // Handle extra data (cliff faces, etc.) if present.
    if has_extra {
        // Native draws covered (nonzero) extra colors after the diamond. The
        // flattened handoff therefore overwrites diamond color and carries the
        // matching ExtraZ byte verbatim, including values at or above 32.
        overlay_extra(
            extra_data,
            extra_z_data,
            extra_x,
            extra_y,
            extra_width,
            extra_height,
            &mut pixels,
            &mut depth,
            pixel_width,
            offset_x,
            offset_y,
        )?;
    }

    Ok(TmpTile {
        height,
        terrain_type,
        ramp_type,
        radar_left,
        radar_right,
        pixels,
        depth,
        pixel_width,
        pixel_height,
        relative_extra_y: extra_y,
        offset_x,
        offset_y,
        has_damaged_data,
    })
}

fn invalid_tmp(reason: String) -> AssetError {
    AssetError::InvalidTmpFile { reason }
}

fn zeroed_plane(len: usize, plane: &str) -> Result<Vec<u8>, AssetError> {
    let mut values: Vec<u8> = Vec::new();
    values.try_reserve_exact(len).map_err(|_| {
        invalid_tmp(format!(
            "Decoded {} plane of {} bytes cannot be allocated",
            plane, len
        ))
    })?;
    values.resize(len, 0);
    Ok(values)
}

fn checked_cell_slice<'a>(
    data: &'a [u8],
    cell_offset: usize,
    relative_offset: u32,
    len: usize,
    plane: &str,
) -> Result<&'a [u8], AssetError> {
    let relative_offset: usize = usize::try_from(relative_offset).map_err(|_| {
        invalid_tmp(format!(
            "Stored {} offset {} does not fit usize",
            plane, relative_offset
        ))
    })?;
    let start: usize = cell_offset.checked_add(relative_offset).ok_or_else(|| {
        invalid_tmp(format!(
            "Stored {} offset {} overflows cell base {}",
            plane, relative_offset, cell_offset
        ))
    })?;
    let end: usize = start.checked_add(len).ok_or_else(|| {
        invalid_tmp(format!(
            "Stored {} range at {} with length {} overflows",
            plane, start, len
        ))
    })?;
    data.get(start..end).ok_or_else(|| {
        invalid_tmp(format!(
            "Stored {} range {}..{} extends past file end {}",
            plane,
            start,
            end,
            data.len()
        ))
    })
}

fn diamond_byte_count(tile_width: u32, tile_height: u32) -> Result<usize, AssetError> {
    if tile_height < 2 {
        return Err(invalid_tmp(format!(
            "Tile height {} cannot contain a TMP diamond",
            tile_height
        )));
    }

    let mut total: usize = 0;
    let mut row_width: u32 = DIAMOND_INITIAL_WIDTH;
    let half_minus_one: u32 = tile_height / 2 - 1;
    for j in 0..tile_height {
        if row_width > tile_width {
            return Err(invalid_tmp(format!(
                "Diamond row {} width {} exceeds tile width {}",
                j, row_width, tile_width
            )));
        }
        total = total
            .checked_add(row_width as usize)
            .ok_or_else(|| invalid_tmp("Diamond byte count overflow".to_string()))?;
        if total > MAX_DECODED_TILE_BYTES {
            return Err(invalid_tmp(format!(
                "Tile {}x{} exceeds the {}-byte tile budget",
                tile_width, tile_height, MAX_DECODED_TILE_BYTES
            )));
        }
        if j < half_minus_one {
            row_width = row_width
                .checked_add(DIAMOND_WIDTH_STEP)
                .ok_or_else(|| invalid_tmp("Diamond row width overflow".to_string()))?;
        } else {
            row_width = row_width.saturating_sub(DIAMOND_WIDTH_STEP);
        }
    }
    Ok(total)
}

/// Unpack diamond-shaped tile data into a rectangular pixel buffer.
///
/// Diamond rows expand from 4 pixels wide, growing by 4 each row until the
/// midpoint, then shrinking back.
fn unpack_diamond(
    data: &[u8],
    tile_width: u32,
    tile_height: u32,
    buf: &mut [u8],
    buf_width: u32,
    buf_origin_x: i32,
    buf_origin_y: i32,
) -> Result<(), AssetError> {
    let buf_width: usize = usize::try_from(buf_width)
        .map_err(|_| invalid_tmp("Decoded tile width does not fit usize".to_string()))?;
    if buf_width == 0 || !buf.len().is_multiple_of(buf_width) {
        return Err(invalid_tmp(format!(
            "Decoded pixel buffer length {} does not match width {}",
            buf.len(),
            buf_width
        )));
    }
    let buf_height: usize = buf.len() / buf_width;
    let mut read_pos: usize = 0;
    let mut row_width: u32 = DIAMOND_INITIAL_WIDTH;
    let half_minus_one: u32 = tile_height / 2 - 1;

    for j in 0..tile_height {
        if row_width > 0 {
            let source_end: usize = read_pos
                .checked_add(row_width as usize)
                .ok_or_else(|| invalid_tmp("Diamond source range overflow".to_string()))?;
            let source: &[u8] = data.get(read_pos..source_end).ok_or_else(|| {
                invalid_tmp(format!(
                    "Diamond row {} needs {} bytes at offset {}, but plane is {} bytes",
                    j,
                    row_width,
                    read_pos,
                    data.len()
                ))
            })?;
            let x_start: i64 = i64::from((tile_width - row_width) / 2) - i64::from(buf_origin_x);
            let y: i64 = i64::from(j) - i64::from(buf_origin_y);
            let x_end: i64 = x_start + i64::from(row_width);
            if x_start < 0 || y < 0 || x_end > buf_width as i64 || y >= buf_height as i64 {
                return Err(invalid_tmp(format!(
                    "Diamond row {} falls outside decoded {}x{} buffer",
                    j, buf_width, buf_height
                )));
            }
            let dest: usize = y as usize * buf_width + x_start as usize;
            let end: usize = dest + row_width as usize;
            buf[dest..end].copy_from_slice(source);
            read_pos = source_end;
        }

        if j < half_minus_one {
            row_width += DIAMOND_WIDTH_STEP;
        } else {
            row_width = row_width.saturating_sub(DIAMOND_WIDTH_STEP);
        }
    }

    Ok(())
}

/// Overlay covered extra colors after the diamond and carry matching depth.
fn overlay_extra(
    extra_data: &[u8],
    extra_z_data: Option<&[u8]>,
    extra_x: i32,
    extra_y: i32,
    extra_width: u32,
    extra_height: u32,
    pixels: &mut [u8],
    depth: &mut [u8],
    buf_width: u32,
    buf_origin_x: i32,
    buf_origin_y: i32,
) -> Result<(), AssetError> {
    let buf_width: usize = usize::try_from(buf_width)
        .map_err(|_| invalid_tmp("Decoded tile width does not fit usize".to_string()))?;
    if buf_width == 0 || pixels.len() != depth.len() || !pixels.len().is_multiple_of(buf_width) {
        return Err(invalid_tmp(
            "Decoded color/depth buffers have inconsistent dimensions".to_string(),
        ));
    }
    let buf_height: usize = pixels.len() / buf_width;
    for ey in 0..extra_height {
        for ex in 0..extra_width {
            let src_idx: usize = ey as usize * extra_width as usize + ex as usize;
            let val: u8 = extra_data[src_idx];
            if val == 0 {
                continue;
            }
            let bx: i64 = i64::from(extra_x) + i64::from(ex) - i64::from(buf_origin_x);
            let by: i64 = i64::from(extra_y) + i64::from(ey) - i64::from(buf_origin_y);
            if bx < 0 || by < 0 || bx >= buf_width as i64 || by >= buf_height as i64 {
                return Err(invalid_tmp(format!(
                    "Extra pixel ({},{}) falls outside decoded {}x{} buffer",
                    ex, ey, buf_width, buf_height
                )));
            }
            let dest: usize = by as usize * buf_width + bx as usize;
            pixels[dest] = val;
            if let Some(extra_z_data) = extra_z_data {
                depth[dest] = extra_z_data[src_idx];
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 52-byte cell header with no pixel payload behind it.
    fn header_only_cell(extra: u32) -> Vec<u8> {
        let mut data = vec![0u8; TILE_HEADER_SIZE];
        data[36..40].copy_from_slice(&extra.to_le_bytes());
        data
    }

    #[test]
    fn oversized_header_dimensions_reject_on_the_budget_not_an_allocation() {
        // 65536x65536 would ask for 8 GiB of zeroed planes; the diamond
        // accumulator must hit the tile budget before any plane is reserved.
        let data = header_only_cell(0);
        let err = parse_tile_cell(&data, 0, 0x1_0000, 0x1_0000)
            .expect_err("dimensions without payload must not decode");
        let AssetError::InvalidTmpFile { reason } = err else {
            panic!("expected a TMP parse error: {err:?}");
        };
        assert!(reason.contains("budget"), "unexpected reason: {reason}");
    }
}
