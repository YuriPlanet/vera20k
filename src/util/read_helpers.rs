//! Little-endian binary reading helpers shared across asset parsers and
//! wire decoders.
//!
//! RA2 asset formats (.mix, .shp, .vxl, .pal, .tmp, .hva) all use
//! little-endian byte ordering for multi-byte integers. These helpers read
//! from raw byte slices and return `None` past the end, so a decoder's bounds
//! check and its read are one operation: a malformed file cannot pass one and
//! panic in the other.
//!
//! ## Why not nom or byteorder?
//! These one-line helpers are simpler than pulling in a parsing combinator
//! for basic "read u16 at offset" operations. They keep the calling code
//! readable: `read_u16_le(data, 4)` vs nom parser chains.
//!
//! ## Dependency rules
//! - Part of util/ — no dependencies on game modules.
//! - Any asset parser or decoder may use these.

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

/// Read `N` bytes at `offset`, or `None` when the slice ends first.
fn read_array<const N: usize>(data: &[u8], offset: usize) -> Option<[u8; N]> {
    data.get(offset..)?.first_chunk::<N>().copied()
}

/// The `N` bytes at `AT` of a fixed-size buffer. A field that would run past
/// the buffer's end fails the build, so the read has no failure to handle.
pub fn array_at<const AT: usize, const N: usize, const L: usize>(bytes: &[u8; L]) -> [u8; N] {
    const { assert!(AT + N <= L, "a fixed field past the end of its buffer") };
    std::array::from_fn(|index| bytes[AT + index])
}

/// Read a little-endian u16 at `offset`, or `None` when the slice ends first.
pub fn read_u16_le(data: &[u8], offset: usize) -> Option<u16> {
    read_array(data, offset).map(u16::from_le_bytes)
}

/// Read a little-endian u32 at `offset`, or `None` when the slice ends first.
pub fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    read_array(data, offset).map(u32::from_le_bytes)
}

/// Read a little-endian i32 at `offset`, or `None` when the slice ends first.
///
/// Used by the MIX archive parser for file ID hashes (which are signed).
pub fn read_i32_le(data: &[u8], offset: usize) -> Option<i32> {
    read_array(data, offset).map(i32::from_le_bytes)
}

/// Read a little-endian f32 at `offset`, or `None` when the slice ends first.
///
/// Used by VXL/HVA parsers for scale factors, bounds, and transform matrices.
pub fn read_f32_le(data: &[u8], offset: usize) -> Option<f32> {
    read_array(data, offset).map(f32::from_le_bytes)
}

#[cfg(all(test, tests_util))]
mod tests {
    use super::*;

    #[test]
    fn test_read_u16_le() {
        // 0x0201 in little-endian = bytes [0x01, 0x02]
        let data: &[u8] = &[0x01, 0x02, 0xFF];
        assert_eq!(read_u16_le(data, 0), Some(0x0201));
        assert_eq!(read_u16_le(data, 2), None);
        assert_eq!(read_u16_le(data, usize::MAX), None);
    }

    #[test]
    fn test_read_i32_le_negative() {
        // -1 in two's complement = 0xFFFFFFFF = bytes [0xFF, 0xFF, 0xFF, 0xFF]
        let data: &[u8] = &[0xFF, 0xFF, 0xFF, 0xFF];
        assert_eq!(read_i32_le(data, 0), Some(-1));
        assert_eq!(read_i32_le(data, 1), None);
    }

    #[test]
    fn test_read_f32_le() {
        // 1.0f32 in little-endian = 0x3F800000 = bytes [0x00, 0x00, 0x80, 0x3F]
        let data: &[u8] = &[0x00, 0x00, 0x80, 0x3F];
        let val: f32 = read_f32_le(data, 0).unwrap();
        assert!((val - 1.0).abs() < f32::EPSILON);
    }
}
