//! LZO1X decompressor for IsoMapPack5 terrain data in RA2 .map files, and
//! the chunk writer for the LAN `Preview.bin`.
//!
//! Decompression is a full LZO1X decoder; compression writes literal runs only. The algorithm is a
//! variant of LZ77 that uses a state machine driven by command bytes. Each
//! command specifies either a literal run (copy bytes from input) or a match
//! copy (back-reference into already-decompressed output).
//!
//! ## Chunk format
//! IsoMapPack5 data is stored as repeating chunks:
//! `[u16 src_len][u16 dst_len][compressed_bytes]` until all input is consumed.
//!
//! ## Dependency rules
//! - Part of util/ — no dependencies on game modules.

use std::fmt;

/// Errors during LZO decompression.
#[derive(Debug)]
pub enum LzoError {
    InputTruncated,
    OutputOverflow,
    InvalidBackRef { distance: usize, output_pos: usize },
}

impl fmt::Display for LzoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LzoError::InputTruncated => write!(f, "LZO: input truncated"),
            LzoError::OutputOverflow => write!(f, "LZO: output buffer overflow"),
            LzoError::InvalidBackRef {
                distance,
                output_pos,
            } => {
                write!(
                    f,
                    "LZO: back-reference distance {} at output pos {}",
                    distance, output_pos
                )
            }
        }
    }
}

impl std::error::Error for LzoError {}

/// Decompress LZO1X-compressed data into `dst`.
/// Returns the number of bytes written to `dst`.
pub fn lzo1x_decompress(src: &[u8], dst: &mut [u8]) -> Result<usize, LzoError> {
    let mut ip: usize = 0; // input position
    let mut op: usize = 0; // output position

    if src.is_empty() {
        return Ok(0);
    }

    // --- First byte special handling ---
    let mut cmd: u8 = read_byte(src, &mut ip)?;
    let mut state: usize;

    if cmd > 17 {
        // Literal run of (cmd - 17) bytes.
        let count: usize = (cmd - 17) as usize;
        copy_literals(src, &mut ip, dst, &mut op, count)?;
        if count < 4 {
            state = count;
            cmd = read_byte(src, &mut ip)?;
            // Fall into match handling below.
        } else {
            // >= 4 literals copied, read next cmd for first_literal_run path.
            state = 4;
            cmd = read_byte(src, &mut ip)?;
            if cmd < 16 {
                // M1' match: distance with +2048 offset.
                let dist: usize =
                    ((cmd >> 2) as usize) + ((read_byte(src, &mut ip)? as usize) << 2) + 2049;
                copy_match(dst, op, dist, 3)?;
                op += 3;
                state = (cmd & 3) as usize;
                if state > 0 {
                    copy_literals(src, &mut ip, dst, &mut op, state)?;
                }
                cmd = read_byte(src, &mut ip)?;
                // Now in the main match loop with state set.
            }
            // If cmd >= 16, fall through to match handling.
        }
    } else {
        state = 0;
        // cmd is the first instruction, fall into main loop.
    }

    // --- Main loop ---
    loop {
        if cmd < 16 {
            if state == 0 {
                // Long literal run.
                let mut length: usize = cmd as usize;
                if length == 0 {
                    length = 15;
                    length += read_vle(src, &mut ip)?;
                }
                length += 3;
                copy_literals(src, &mut ip, dst, &mut op, length)?;
                // First-literal-run path: next cmd may be M1' match.
                cmd = read_byte(src, &mut ip)?;
                if cmd < 16 {
                    let dist: usize =
                        ((cmd >> 2) as usize) + ((read_byte(src, &mut ip)? as usize) << 2) + 2049;
                    copy_match(dst, op, dist, 3)?;
                    op += 3;
                    state = (cmd & 3) as usize;
                    if state > 0 {
                        copy_literals(src, &mut ip, dst, &mut op, state)?;
                    }
                    cmd = read_byte(src, &mut ip)?;
                    continue;
                }
                // cmd >= 16, fall through to match handling.
            } else {
                // M1 short match (state >= 1): copy 2 bytes.
                let dist: usize =
                    ((cmd >> 2) as usize) + ((read_byte(src, &mut ip)? as usize) << 2) + 1;
                copy_match(dst, op, dist, 2)?;
                op += 2;
                state = (cmd & 3) as usize;
                if state > 0 {
                    copy_literals(src, &mut ip, dst, &mut op, state)?;
                }
                cmd = read_byte(src, &mut ip)?;
                continue;
            }
        }

        // --- Match instructions (cmd >= 16) ---
        let (match_len, dist): (usize, usize);

        if cmd >= 64 {
            // M2: short match, 1..2048 distance, 3..8 length.
            let len_part: usize = (cmd >> 5) as usize; // 2..7
            let dist_lo: usize = ((cmd >> 2) & 7) as usize;
            let dist_hi: usize = read_byte(src, &mut ip)? as usize;
            dist = (dist_hi << 3) + dist_lo + 1;
            match_len = len_part + 1; // 3..8
        } else if cmd >= 32 {
            // M3: medium match, 1..16384 distance.
            let mut length: usize = (cmd & 31) as usize;
            if length == 0 {
                length = 31;
                length += read_vle(src, &mut ip)?;
            }
            match_len = length + 2;
            let word: u16 = read_u16_le_lzo(src, &mut ip)?;
            dist = ((word >> 2) as usize) + 1;
            state = (word & 3) as usize;
            copy_match(dst, op, dist, match_len)?;
            op += match_len;
            if state > 0 {
                copy_literals(src, &mut ip, dst, &mut op, state)?;
            }
            cmd = read_byte(src, &mut ip)?;
            continue;
        } else {
            // M4: long match (16..31), or end-of-stream.
            let mut length: usize = (cmd & 7) as usize;
            if length == 0 {
                length = 7;
                length += read_vle(src, &mut ip)?;
            }
            match_len = length + 2;
            let high_dist: usize = ((cmd as usize) & 8) << 11;
            let word: u16 = read_u16_le_lzo(src, &mut ip)?;
            let low_dist: usize = (word >> 2) as usize;

            // End-of-stream: high_dist + low_dist == 0.
            if high_dist == 0 && low_dist == 0 {
                return Ok(op);
            }
            dist = high_dist + low_dist + 16384;
            state = (word & 3) as usize;
            copy_match(dst, op, dist, match_len)?;
            op += match_len;
            if state > 0 {
                copy_literals(src, &mut ip, dst, &mut op, state)?;
            }
            cmd = read_byte(src, &mut ip)?;
            continue;
        }

        // M2 match — state (literal count after match) is in the command byte's
        // low 2 bits, NOT the distance byte. M2 format: [LLL][DDD][SS] where
        // SS = state, DDD = dist_lo, LLL = length.
        state = (cmd & 3) as usize;
        copy_match(dst, op, dist, match_len)?;
        op += match_len;
        if state > 0 {
            copy_literals(src, &mut ip, dst, &mut op, state)?;
        }
        cmd = read_byte(src, &mut ip)?;
    }
}

/// Decompress IsoMapPack5 chunk-framed LZO data.
///
/// Chunk format: repeating `[u16 src_len][u16 dst_len][compressed_bytes]`.
/// Stops when all input bytes are consumed. The caller supplies the maximum
/// total decoded size; every advertised chunk is checked before allocation.
pub fn decompress_chunks(data: &[u8], maximum_output: usize) -> Result<Vec<u8>, LzoError> {
    let mut output: Vec<u8> = Vec::new();
    let mut pos: usize = 0;

    while pos + 4 <= data.len() {
        let src_len: usize = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        let dst_len: usize = u16::from_le_bytes([data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;

        if src_len == 0 && dst_len == 0 {
            break;
        }
        if pos + src_len > data.len() {
            return Err(LzoError::InputTruncated);
        }

        let start = output.len();
        let end = start
            .checked_add(dst_len)
            .filter(|&end| end <= maximum_output)
            .ok_or(LzoError::OutputOverflow)?;
        output
            .try_reserve_exact(dst_len)
            .map_err(|_| LzoError::OutputOverflow)?;
        output.resize(end, 0);
        let written = lzo1x_decompress(&data[pos..pos + src_len], &mut output[start..end])?;
        output.truncate(start + written);
        pos += src_len;
    }

    Ok(output)
}

/// Write `data` as `LZOPipe(0, block_size)` does (`0x0055C350`, flushed by
/// `0x0055C5E0`): blocks of at most `block_size` bytes, each behind
/// `[u16 compressed_len][u16 uncompressed_len]`.
///
/// RESIDUAL: native compresses each block with LZO1X-1 (`0x0055BB90`); this
/// writes valid LZO1X streams of literal runs and back-references found by
/// its own greedy search, which decode to the same bytes but are not the
/// bytes native writes.
pub fn compress_chunks(data: &[u8], block_size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 2 + 16);
    // A block's stream must fit the frame's 16-bit length. As one literal
    // run it does whatever the block holds (its bytes, a run header and the
    // end marker). With matches it can grow past the block, where short
    // matches from far back cost as much as the bytes they stand for; the
    // literal run is then written when it is shorter.
    for block in data.chunks(block_size.clamp(1, 0xFE00)) {
        let mut stream = lzo1x_stream(block, true);
        if stream.len() > block.len() {
            let literal = lzo1x_stream(block, false);
            if literal.len() < stream.len() {
                stream = literal;
            }
        }
        out.extend_from_slice(&(stream.len() as u16).to_le_bytes());
        out.extend_from_slice(&(block.len() as u16).to_le_bytes());
        out.extend_from_slice(&stream);
    }
    out
}

/// The run-length extension [`read_vle`] reads back: zero bytes of 255
/// each, then the rest (1..=255).
fn write_vle(out: &mut Vec<u8>, mut extra: usize) {
    while extra > 255 {
        out.push(0);
        extra -= 255;
    }
    out.push(extra as u8);
}

/// One LZO1X stream of `block`, then the end marker; without
/// `search_matches`, the block as one literal run.
///
/// A greedy search over a table of the last place each three-byte group was
/// seen. Each instruction is one [`lzo1x_decompress`] reads:
/// - Literals before the first match: `17 + count` up to 238 bytes, else
///   the long run `0` with its extension.
/// - Literals after a match: one to three ride in the match's two state
///   bits; four or more are a run `count - 3` (to 18) or `0` with its
///   extension.
/// - A match of 3..=8 bytes within 2048 is M2 (`cmd >= 64`); any other is
///   M3 (`32..64`), whose distance reaches 16384; nothing farther back is
///   matched.
///
/// A run of four or more is always followed by a match or the end marker,
/// never by a `cmd < 16`, which the decoder would read as an M1 match.
fn lzo1x_stream(block: &[u8], search_matches: bool) -> Vec<u8> {
    const HASH_BITS: u32 = 13;
    const MIN_MATCH: usize = 3;
    const MAX_DISTANCE: usize = 16384;
    let hash = |at: usize| {
        let group =
            u32::from(block[at]) | u32::from(block[at + 1]) << 8 | u32::from(block[at + 2]) << 16;
        (group.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
    };
    let mut seen = vec![usize::MAX; 1 << HASH_BITS];
    let mut out = Vec::with_capacity(block.len() / 2 + 16);
    // Where the last match keeps its state bits, while literals may still
    // ride in them.
    let mut state_at: Option<usize> = None;
    let mut literals_from = 0;
    let mut at = 0;

    let write_literals = |out: &mut Vec<u8>, state_at: &mut Option<usize>, run: &[u8]| {
        let count = run.len();
        if count == 0 {
            return;
        }
        match *state_at {
            Some(state) if count <= 3 => out[state] |= count as u8,
            _ if out.is_empty() && count <= 238 => out.push(17 + count as u8),
            _ if out.is_empty() || count > 18 => {
                out.push(0);
                write_vle(out, count - 18);
            }
            _ => out.push((count - 3) as u8),
        }
        out.extend_from_slice(run);
        *state_at = None;
    };

    while search_matches && at + MIN_MATCH <= block.len() {
        let slot = hash(at);
        let candidate = seen[slot];
        seen[slot] = at;
        let matches = candidate != usize::MAX
            && at - candidate <= MAX_DISTANCE
            && block[candidate..candidate + MIN_MATCH] == block[at..at + MIN_MATCH];
        if !matches {
            at += 1;
            continue;
        }
        let distance = at - candidate;
        let mut length = MIN_MATCH;
        while at + length < block.len() && block[candidate + length] == block[at + length] {
            length += 1;
        }
        write_literals(&mut out, &mut state_at, &block[literals_from..at]);
        if length <= 8 && distance <= 2048 {
            // M2: [length - 1 : 3][distance low : 3][state : 2], distance high.
            state_at = Some(out.len());
            out.push(((length - 1) as u8) << 5 | (((distance - 1) & 7) as u8) << 2);
            out.push(((distance - 1) >> 3) as u8);
        } else {
            // M3: 32 | (length - 2), extended past 31; then the distance and
            // the state in a little-endian word.
            if length - 2 <= 31 {
                out.push(32 | (length - 2) as u8);
            } else {
                out.push(32);
                write_vle(&mut out, length - 2 - 31);
            }
            let word = ((distance - 1) << 2) as u16;
            state_at = Some(out.len());
            out.extend_from_slice(&word.to_le_bytes());
        }
        // Remember the groups the match covers, so later data finds them.
        for covered in at + 1..(at + length).min(block.len().saturating_sub(MIN_MATCH - 1)) {
            seen[hash(covered)] = covered;
        }
        at += length;
        literals_from = at;
    }
    write_literals(&mut out, &mut state_at, &block[literals_from..]);
    out.extend_from_slice(&[0x11, 0x00, 0x00]);
    out
}

// --- Internal helpers ---

/// Read a single byte, advancing the position.
fn read_byte(src: &[u8], pos: &mut usize) -> Result<u8, LzoError> {
    if *pos >= src.len() {
        return Err(LzoError::InputTruncated);
    }
    let b: u8 = src[*pos];
    *pos += 1;
    Ok(b)
}

/// Read a u16 LE value, advancing the position by 2.
fn read_u16_le_lzo(src: &[u8], pos: &mut usize) -> Result<u16, LzoError> {
    if *pos + 2 > src.len() {
        return Err(LzoError::InputTruncated);
    }
    let val: u16 = u16::from_le_bytes([src[*pos], src[*pos + 1]]);
    *pos += 2;
    Ok(val)
}

/// Read variable-length extension: skip zero bytes (each adds 255) then add the
/// first non-zero byte.
fn read_vle(src: &[u8], pos: &mut usize) -> Result<usize, LzoError> {
    let mut extra: usize = 0;
    loop {
        let b: u8 = read_byte(src, pos)?;
        if b != 0 {
            return Ok(extra + b as usize);
        }
        extra += 255;
    }
}

/// Copy `count` literal bytes from input to output.
fn copy_literals(
    src: &[u8],
    ip: &mut usize,
    dst: &mut [u8],
    op: &mut usize,
    count: usize,
) -> Result<(), LzoError> {
    if *ip + count > src.len() {
        return Err(LzoError::InputTruncated);
    }
    if *op + count > dst.len() {
        return Err(LzoError::OutputOverflow);
    }
    dst[*op..*op + count].copy_from_slice(&src[*ip..*ip + count]);
    *ip += count;
    *op += count;
    Ok(())
}

/// Copy `count` bytes from a back-reference in the output buffer.
/// Must copy byte-by-byte because source and destination may overlap.
fn copy_match(dst: &mut [u8], op: usize, distance: usize, count: usize) -> Result<(), LzoError> {
    if distance > op {
        return Err(LzoError::InvalidBackRef {
            distance,
            output_pos: op,
        });
    }
    if op + count > dst.len() {
        return Err(LzoError::OutputOverflow);
    }
    let src_start = op - distance;
    for i in 0..count {
        dst[op + i] = dst[src_start + i];
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal_only_stream() {
        // A stream with just literals: first byte = 17 + count, then literal bytes,
        // then end-of-stream marker (0x11, 0x00, 0x00).
        let input: Vec<u8> = vec![
            17 + 5, // 5 literal bytes follow
            b'H',
            b'e',
            b'l',
            b'l',
            b'o',
            0x11,
            0x00,
            0x00, // end of stream
        ];
        let mut output: [u8; 64] = [0u8; 64];
        let written: usize = lzo1x_decompress(&input, &mut output).expect("Should decompress");
        assert_eq!(written, 5);
        assert_eq!(&output[..5], b"Hello");
    }

    #[test]
    fn written_chunks_decode_back_at_every_run_length_encoding() {
        for len in [
            1, 2, 3, 4, 18, 19, 238, 239, 255, 529, 0x2000, 0x2001, 20_000,
        ] {
            let data: Vec<u8> = (0..len).map(|i| (i * 7 + i / 3) as u8).collect();
            let chunks = compress_chunks(&data, 0x2000);
            assert_eq!(
                decompress_chunks(&chunks, data.len()).expect("decodes"),
                data,
                "{len}"
            );
            // Every block but the last is a full 0x2000 bytes.
            let uncompressed = u16::from_le_bytes([chunks[2], chunks[3]]) as usize;
            assert_eq!(uncompressed, len.min(0x2000));
        }
        assert!(compress_chunks(&[], 0x2000).is_empty());
    }

    /// Repeating data comes back whole and smaller: map cell records, one
    /// long run, short repeats with one to three bytes between them, and a
    /// repeat that begins at the block's second byte.
    #[test]
    fn written_chunks_shrink_repeating_data_and_decode_back() {
        // 11-byte IsoMapPack5-like records: coordinates count up, the rest
        // repeats.
        let records: Vec<u8> = (0..3000u16)
            .flat_map(|cell| {
                let [x, y] = cell.to_le_bytes();
                [x, y, 30, 0, 12, 0, 0, 0, (cell % 3) as u8, 2, 0]
            })
            .collect();
        let run = vec![0x5Au8; 20_000];
        let stitched: Vec<u8> = (0..4000usize)
            .flat_map(|i| {
                let gap = i % 4;
                let mut piece = b"ABCDEFGH".to_vec();
                piece.extend(std::iter::repeat_n((i * 31) as u8, gap));
                piece
            })
            .collect();
        let early = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaab".to_vec();
        for (name, data, shrinks_to) in [
            ("records", &records, 2),
            ("run", &run, 100),
            ("stitched", &stitched, 2),
            ("early", &early, 2),
        ] {
            let chunks = compress_chunks(data, 0x2000);
            assert_eq!(
                &decompress_chunks(&chunks, data.len()).expect("decodes"),
                data,
                "{name}"
            );
            assert!(
                chunks.len() * shrinks_to < data.len(),
                "{name}: {} bytes from {}",
                chunks.len(),
                data.len()
            );
        }
    }

    /// A full-size block of three-byte repeats from more than 2048 bytes
    /// back, each before four new bytes: those matches cost as much as the
    /// bytes they stand for, yet the block still fits its frame's 16-bit
    /// length and decodes back.
    #[test]
    fn a_block_of_costly_matches_still_fits_its_frame() {
        let mut state = 0x2545_F491u32;
        let mut noise = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        };
        let mut data: Vec<u8> = (0..4096).map(|_| noise()).collect();
        while data.len() < 0xFE00 {
            let from = data.len() - 2100;
            data.extend_from_within(from..from + 3);
            data.extend((0..4).map(|_| noise()));
        }
        data.truncate(0xFE00);
        let chunks = compress_chunks(&data, 0xFE00);
        assert_eq!(decompress_chunks(&chunks, data.len()).ok(), Some(data));
    }

    #[test]
    fn test_chunk_wrapper() {
        // Single chunk containing a literal-only LZO stream.
        let lzo_data: Vec<u8> = vec![17 + 3, b'A', b'B', b'C', 0x11, 0x00, 0x00];
        let mut chunks: Vec<u8> = Vec::new();
        let src_len: u16 = lzo_data.len() as u16;
        let dst_len: u16 = 3;
        chunks.extend_from_slice(&src_len.to_le_bytes());
        chunks.extend_from_slice(&dst_len.to_le_bytes());
        chunks.extend_from_slice(&lzo_data);

        let result: Vec<u8> = decompress_chunks(&chunks, 3).expect("Should decompress chunks");
        assert_eq!(result, b"ABC");
    }
}
