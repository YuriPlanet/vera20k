//! LCW (Lempel-Castle-Welch) / Format80 decompression.
//!
//! Westwood's proprietary compression used for overlay data in RA2/TS maps.
//! The [OverlayPack] and [OverlayDataPack] sections use LCW compression
//! (different from the LZO used by [IsoMapPack5]).
//!
//! ## Command byte encoding
//! - `0x00-0x7F` (bit7=0): 2-byte relative back-reference copy
//! - `0x80` (count=0): end-of-data marker
//! - `0x80-0xBF` (bit7=1, bit6=0): literal copy from source
//! - `0xC0-0xFD`: 3-byte absolute copy from earlier output
//! - `0xFE`: RLE fill (repeat a byte N times)
//! - `0xFF`: 5-byte large absolute copy from earlier output
//!
//! ## Reference
//! Verified against actual RA2 map data.
//!
//! ## Dependency rules
//! - Part of util/ — no game dependencies.

/// Errors during LCW decompression.
#[derive(Debug)]
pub enum LcwError {
    /// Input data ended unexpectedly.
    InputTruncated,
    /// Back-reference points before the start of the output buffer.
    InvalidBackRef { src_index: usize, dest_index: usize },
    /// Output buffer overflow.
    OutputOverflow { needed: usize, capacity: usize },
}

impl std::fmt::Display for LcwError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LcwError::InputTruncated => write!(f, "LCW input data truncated"),
            LcwError::InvalidBackRef {
                src_index,
                dest_index,
            } => {
                write!(
                    f,
                    "LCW invalid back-ref: src={} dest={}",
                    src_index, dest_index
                )
            }
            LcwError::OutputOverflow { needed, capacity } => {
                write!(
                    f,
                    "LCW output overflow: need {} bytes, buffer is {}",
                    needed, capacity
                )
            }
        }
    }
}

impl std::error::Error for LcwError {}

/// Decompress LCW/Format80 data into a pre-sized output buffer.
///
/// Returns the number of bytes written to `dest`.
/// Decompression stops when the end marker (0x80) is reached or input is exhausted.
pub fn lcw_decompress(src: &[u8], dest: &mut [u8]) -> Result<usize, LcwError> {
    let mut si: usize = 0; // source index
    let mut di: usize = 0; // dest index

    loop {
        if si >= src.len() {
            return Ok(di);
        }
        let cmd: u8 = src[si];
        si += 1;

        if (cmd & 0x80) == 0 {
            // Case 2: Relative back-reference copy (0x00-0x7F).
            // 2-byte command: count from bits 4-6, distance from bits 0-3 + next byte.
            if si >= src.len() {
                return Err(LcwError::InputTruncated);
            }
            let second: u8 = src[si];
            si += 1;

            let count: usize = (((cmd & 0x70) >> 4) + 3) as usize;
            let distance: usize = (((cmd & 0x0F) as usize) << 8) | second as usize;

            if distance > di {
                return Err(LcwError::InvalidBackRef {
                    src_index: di.wrapping_sub(distance),
                    dest_index: di,
                });
            }
            if di + count > dest.len() {
                return Err(LcwError::OutputOverflow {
                    needed: di + count,
                    capacity: dest.len(),
                });
            }

            replicate_previous(dest, di, di - distance, count);
            di += count;
        } else if (cmd & 0x40) == 0 {
            // Case 1: Literal copy (0x80-0xBF).
            // Lower 6 bits = byte count. Count 0 = end marker.
            let count: usize = (cmd & 0x3F) as usize;
            if count == 0 {
                return Ok(di); // End of data.
            }
            if si + count > src.len() {
                return Err(LcwError::InputTruncated);
            }
            if di + count > dest.len() {
                return Err(LcwError::OutputOverflow {
                    needed: di + count,
                    capacity: dest.len(),
                });
            }
            dest[di..di + count].copy_from_slice(&src[si..si + count]);
            si += count;
            di += count;
        } else {
            // 0xC0-0xFF: extended commands.
            let lower6: u8 = cmd & 0x3F;

            if lower6 == 0x3E {
                // Case 4: RLE fill (cmd == 0xFE).
                // Next 2 bytes = count (u16 LE), then 1 byte = fill value.
                if si + 3 > src.len() {
                    return Err(LcwError::InputTruncated);
                }
                let count: usize = u16::from_le_bytes([src[si], src[si + 1]]) as usize;
                let value: u8 = src[si + 2];
                si += 3;

                if di + count > dest.len() {
                    return Err(LcwError::OutputOverflow {
                        needed: di + count,
                        capacity: dest.len(),
                    });
                }
                dest[di..di + count].fill(value);
                di += count;
            } else if lower6 == 0x3F {
                // Case 5: Large absolute copy (cmd == 0xFF).
                // Next 2 bytes = count (u16 LE), then 2 bytes = absolute source position (u16 LE).
                if si + 4 > src.len() {
                    return Err(LcwError::InputTruncated);
                }
                let count: usize = u16::from_le_bytes([src[si], src[si + 1]]) as usize;
                let abs_pos: usize = u16::from_le_bytes([src[si + 2], src[si + 3]]) as usize;
                si += 4;

                if abs_pos >= di {
                    return Err(LcwError::InvalidBackRef {
                        src_index: abs_pos,
                        dest_index: di,
                    });
                }
                if di + count > dest.len() {
                    return Err(LcwError::OutputOverflow {
                        needed: di + count,
                        capacity: dest.len(),
                    });
                }
                for i in 0..count {
                    dest[di + i] = dest[abs_pos + i];
                }
                di += count;
            } else {
                // Case 3: Short absolute copy (0xC0-0xFD, excluding 0xFE/0xFF).
                // count = lower6 + 3, then 2 bytes = absolute source position (u16 LE).
                let count: usize = lower6 as usize + 3;
                if si + 2 > src.len() {
                    return Err(LcwError::InputTruncated);
                }
                let abs_pos: usize = u16::from_le_bytes([src[si], src[si + 1]]) as usize;
                si += 2;

                if abs_pos >= di {
                    return Err(LcwError::InvalidBackRef {
                        src_index: abs_pos,
                        dest_index: di,
                    });
                }
                if di + count > dest.len() {
                    return Err(LcwError::OutputOverflow {
                        needed: di + count,
                        capacity: dest.len(),
                    });
                }
                for i in 0..count {
                    dest[di + i] = dest[abs_pos + i];
                }
                di += count;
            }
        }
    }
}

/// Copy bytes from earlier in the output buffer, handling overlapping regions.
/// When distance is 1, replicates the previous byte (RLE-like behavior).
fn replicate_previous(dest: &mut [u8], di: usize, src_idx: usize, count: usize) {
    if di - src_idx == 1 {
        // Distance 1: repeat the single previous byte.
        let val: u8 = dest[di - 1];
        dest[di..di + count].fill(val);
    } else {
        // General case: byte-by-byte copy (handles overlapping regions).
        for i in 0..count {
            dest[di + i] = dest[src_idx + i];
        }
    }
}

/// Decompress chunked LCW data (same chunk format as LZO).
///
/// Chunk layout: [u16 LE src_len][u16 LE dst_len][src_len bytes of LCW data]
/// Chunks repeat until all data is consumed.
pub fn decompress_chunks(data: &[u8]) -> Result<Vec<u8>, LcwError> {
    let mut result: Vec<u8> = Vec::new();
    let mut offset: usize = 0;

    while offset + 4 <= data.len() {
        let src_len: usize = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
        let dst_len: usize = u16::from_le_bytes([data[offset + 2], data[offset + 3]]) as usize;
        offset += 4;

        if offset + src_len > data.len() {
            return Err(LcwError::InputTruncated);
        }

        let mut temp: Vec<u8> = vec![0u8; dst_len.max(262_144)];
        let written: usize = lcw_decompress(&data[offset..offset + src_len], &mut temp)?;

        // Use dst_len bytes (the expected output), not necessarily all written bytes.
        let copy_len: usize = dst_len.min(written);
        result.extend_from_slice(&temp[..copy_len]);
        offset += src_len;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal_copy() {
        // Case 1: 0x83 = literal copy of 3 bytes, then 0x80 = end.
        let src: &[u8] = &[0x83, 0xAA, 0xBB, 0xCC, 0x80];
        let mut dest: [u8; 16] = [0; 16];
        let n: usize = lcw_decompress(src, &mut dest).unwrap();
        assert_eq!(n, 3);
        assert_eq!(&dest[..3], &[0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn test_rle_fill() {
        // Case 4: 0xFE, count=5 (LE), fill=0x42, then 0x80 = end.
        let src: &[u8] = &[0xFE, 0x05, 0x00, 0x42, 0x80];
        let mut dest: [u8; 16] = [0; 16];
        let n: usize = lcw_decompress(src, &mut dest).unwrap();
        assert_eq!(n, 5);
        assert_eq!(&dest[..5], &[0x42, 0x42, 0x42, 0x42, 0x42]);
    }

    #[test]
    fn test_relative_backref() {
        // Write 4 literal bytes, then Case 2: relative back-ref.
        // 0x84 = literal 4 bytes: [10, 20, 30, 40]
        // Then cmd=0x00, second=0x04 → count=3, distance=4 → copy from offset 0
        // Result: [10, 20, 30, 40, 10, 20, 30]
        let src: &[u8] = &[0x84, 10, 20, 30, 40, 0x00, 0x04, 0x80];
        let mut dest: [u8; 16] = [0; 16];
        let n: usize = lcw_decompress(src, &mut dest).unwrap();
        assert_eq!(n, 7);
        assert_eq!(&dest[..7], &[10, 20, 30, 40, 10, 20, 30]);
    }

    #[test]
    fn test_rle_via_distance_one() {
        // Write 1 literal byte [0xFF], then relative back-ref with distance=1 count=5.
        // cmd=0x20 (count=((0x20 & 0x70)>>4)+3 = 5), second=0x01 (distance=1).
        let src: &[u8] = &[0x81, 0xFF, 0x20, 0x01, 0x80];
        let mut dest: [u8; 16] = [0; 16];
        let n: usize = lcw_decompress(src, &mut dest).unwrap();
        assert_eq!(n, 6);
        assert_eq!(&dest[..6], &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn test_chunk_decompression() {
        // Build a chunk: src_len=5, dst_len=3, then LCW data (3 literal bytes + end).
        let lcw_data: &[u8] = &[0x83, 0xAA, 0xBB, 0xCC, 0x80];
        let mut chunk: Vec<u8> = Vec::new();
        chunk.extend_from_slice(&5u16.to_le_bytes()); // src_len
        chunk.extend_from_slice(&3u16.to_le_bytes()); // dst_len
        chunk.extend_from_slice(lcw_data);

        let result: Vec<u8> = decompress_chunks(&chunk).unwrap();
        assert_eq!(result, vec![0xAA, 0xBB, 0xCC]);
    }
}
