//! Little-endian binary reading helpers shared across asset parsers.
//!
//! RA2 asset formats (.mix, .shp, .vxl, .pal, .tmp, .hva) all use
//! little-endian byte ordering for multi-byte integers. These helpers
//! provide safe, bounds-checked reading from raw byte slices.
//!
//! ## Why not nom or byteorder?
//! These one-line helpers are simpler than pulling in a parsing combinator
//! for basic "read u16 at offset" operations. They keep the calling code
//! readable: `read_u16_le(data, 4)` vs nom parser chains.
//!
//! ## Dependency rules
//! - Part of util/ — no dependencies on game modules.
//! - Any asset parser may use these.

/// Read a little-endian u16 from a byte slice at the given offset.
///
/// # Panics
/// Panics if `offset + 2 > data.len()`. Callers should bounds-check first.
pub fn read_u16_le(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}

/// Read a little-endian u32 from a byte slice at the given offset.
///
/// # Panics
/// Panics if `offset + 4 > data.len()`. Callers should bounds-check first.
pub fn read_u32_le(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

/// Read a little-endian i32 from a byte slice at the given offset.
///
/// Used by the MIX archive parser for file ID hashes (which are signed).
///
/// # Panics
/// Panics if `offset + 4 > data.len()`. Callers should bounds-check first.
pub fn read_i32_le(data: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

/// Read a little-endian f32 from a byte slice at the given offset.
///
/// Used by VXL/HVA parsers for scale factors, bounds, and transform matrices.
///
/// # Panics
/// Panics if `offset + 4 > data.len()`. Callers should bounds-check first.
pub fn read_f32_le(data: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_u16_le() {
        // 0x0201 in little-endian = bytes [0x01, 0x02]
        let data: &[u8] = &[0x01, 0x02, 0xFF];
        assert_eq!(read_u16_le(data, 0), 0x0201);
    }

    #[test]
    fn test_read_i32_le_negative() {
        // -1 in two's complement = 0xFFFFFFFF = bytes [0xFF, 0xFF, 0xFF, 0xFF]
        let data: &[u8] = &[0xFF, 0xFF, 0xFF, 0xFF];
        assert_eq!(read_i32_le(data, 0), -1);
    }

    #[test]
    fn test_read_f32_le() {
        // 1.0f32 in little-endian = 0x3F800000 = bytes [0x00, 0x00, 0x80, 0x3F]
        let data: &[u8] = &[0x00, 0x00, 0x80, 0x3F];
        let val: f32 = read_f32_le(data, 0);
        assert!((val - 1.0).abs() < f32::EPSILON);
    }
}
