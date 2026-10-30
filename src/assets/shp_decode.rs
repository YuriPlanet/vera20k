//! Row-framed RLE-Zero decompression for SHP(TS) sprite frames.
//!
//! Frame-header byte 8 is a bitfield. When bit 1 is set, the active YR draw
//! path uses this row-framed RLE-Zero grammar; when it is clear, the frame is
//! contiguous raw pixels and is handled by `shp_file`.
//!
//! Each row starts with a self-inclusive little-endian `u16` byte count. The
//! payload is width-driven: a nonzero byte emits that palette index, while
//! `0, count` advances by `count` transparent pixels. Payload tail remaining
//! after the visible width is ignored before advancing to the next row.
//!
//! ## Dependency rules
//! - Part of assets/ - no dependencies on game modules.

use crate::assets::error::AssetError;

fn parse_error(detail: impl Into<String>) -> AssetError {
    AssetError::ParseError {
        format: "SHP".to_string(),
        detail: detail.into(),
    }
}

/// Decode a bit-1-set SHP frame using the active row/RLE-Zero mechanism.
///
/// Returns exactly `width * height` palette indices for well-formed data.
pub fn decode_rle_frame(data: &[u8], width: usize, height: usize) -> Result<Vec<u8>, AssetError> {
    let pixel_count = width
        .checked_mul(height)
        .ok_or_else(|| parse_error("RLE frame dimensions overflow addressable memory"))?;
    // Every produced pixel costs input: a nonzero literal one byte, a zero run
    // two bytes for up to 255 pixels. A frame claiming more pixels than
    // `255 * data.len()/2` has no payload behind its header: reject before the
    // capacity reflects the claim.
    let max_pixels = 255 * (data.len() / 2);
    if pixel_count > max_pixels {
        return Err(parse_error(format!(
            "RLE frame needs {pixel_count} pixels but {} frame bytes cannot produce past {max_pixels}",
            data.len()
        )));
    }
    let mut pixels = Vec::with_capacity(pixel_count);
    let mut offset = 0usize;

    for row in 0..height {
        let row_start = offset;
        let prefix_end = row_start
            .checked_add(2)
            .ok_or_else(|| parse_error(format!("RLE scanline {row} prefix offset overflow")))?;
        if prefix_end > data.len() {
            return Err(parse_error(format!(
                "RLE data truncated at scanline {row} (offset {row_start})"
            )));
        }

        let raw_length = u16::from_le_bytes([data[row_start], data[row_start + 1]]) as usize;
        if raw_length < 2 {
            return Err(parse_error(format!(
                "RLE scanline {row} length {raw_length} is smaller than its 2-byte prefix"
            )));
        }

        let line_end = row_start.checked_add(raw_length).ok_or_else(|| {
            parse_error(format!(
                "RLE scanline {row} declared end overflows address space"
            ))
        })?;
        if line_end > data.len() {
            return Err(parse_error(format!(
                "RLE scanline {row} extends past frame data ({line_end} > {})",
                data.len()
            )));
        }

        offset = prefix_end;
        let mut row_pixels = 0usize;

        // The native row consumer is width-driven. The prefix still owns the
        // next-row address, so declared payload tail is ignored.
        while row_pixels < width {
            if offset >= line_end {
                return Err(parse_error(format!(
                    "RLE scanline {row} under-runs: produced {row_pixels} of {width} pixels"
                )));
            }

            let byte = data[offset];
            offset += 1;
            if byte != 0 {
                pixels.push(byte);
                row_pixels += 1;
                continue;
            }

            if offset >= line_end {
                return Err(parse_error(format!(
                    "RLE scanline {row} ends inside a zero run"
                )));
            }
            let count = data[offset] as usize;
            offset += 1;
            let visible_count = count.min(width - row_pixels);
            pixels.extend(std::iter::repeat_n(0u8, visible_count));
            row_pixels += visible_count;
        }

        offset = line_end;
    }

    Ok(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rle_decode_basic() {
        let data = [6, 0, 0, 1, 5, 3];
        let pixels = decode_rle_frame(&data, 3, 1).expect("decode");
        assert_eq!(pixels, vec![0, 5, 3]);
    }

    #[test]
    fn a_huge_header_pixel_count_cannot_reserve_past_the_frame_payload() {
        // 65535x65535 claims 4.3 billion pixels behind four frame bytes; the
        // expansion cap must reject before the capacity reflects the claim.
        let data = [8u8, 0, 0, 0];
        let err =
            decode_rle_frame(&data, 65535, 65535).expect_err("no payload behind the pixel count");
        let AssetError::ParseError { detail, .. } = err else {
            panic!("expected a SHP parse error: {err:?}");
        };
        assert!(
            detail.contains("cannot produce past"),
            "unexpected detail: {detail}"
        );
    }

    #[test]
    fn truncated_prefix_errors() {
        assert!(decode_rle_frame(&[], 2, 1).is_err());
    }

    #[test]
    fn zero_opcode_without_count_errors() {
        assert!(decode_rle_frame(&[3, 0, 0], 1, 1).is_err());
    }

    #[test]
    fn declared_row_under_run_errors_instead_of_padding() {
        assert!(decode_rle_frame(&[3, 0, 7], 2, 1).is_err());
    }

    #[test]
    fn final_zero_run_overshoot_is_clamped_to_visible_width() {
        let pixels = decode_rle_frame(&[4, 0, 0, 8], 3, 1).expect("overshoot is safe");
        assert_eq!(pixels, vec![0, 0, 0]);
    }

    #[test]
    fn payload_tail_is_ignored_before_the_next_declared_row() {
        let data = [
            6, 0, 9, 8, 0xAA, 0xBB, // row 0: two literals plus ignored tail
            4, 0, 0, 2, // row 1: two transparent pixels
        ];
        let pixels = decode_rle_frame(&data, 2, 2).expect("tail stays inside its row");
        assert_eq!(pixels, vec![9, 8, 0, 0]);
    }
}
