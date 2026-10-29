//! Base64 decoder for IsoMapPack5 and other binary sections in RA2 .map files.
//!
//! Standard alphabet (A-Za-z0-9+/), `=` padding. Whitespace and newlines are
//! skipped since map files split base64 data across numbered INI keys.
//!
//! ## Dependency rules
//! - Part of util/ — no dependencies on game modules.

/// Decode a base64 string to raw bytes.
///
/// Skips any whitespace/newline characters (map files store base64 data across
/// multiple numbered INI lines). Returns an error if invalid characters are found.
pub fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    base64_decode_chunks(&[input])
}

/// Decode Base64 text split across consecutive source chunks.
///
/// Map packs store 127-byte values in separate INI entries. Treating the
/// chunks as one byte stream preserves the native concatenation order without
/// allocating a joined copy first.
pub fn base64_decode_chunks(chunks: &[&str]) -> Result<Vec<u8>, String> {
    let input_len = chunks
        .iter()
        .fold(0_usize, |total, chunk| total.saturating_add(chunk.len()));
    let mut output: Vec<u8> = Vec::with_capacity(input_len.saturating_mul(3) / 4);
    let mut group = [0_u8; 4];
    let mut group_len = 0_usize;
    let mut padding: usize = 0;

    for chunk in chunks {
        for &byte in chunk.as_bytes() {
            if byte.is_ascii_whitespace() {
                continue;
            }
            if byte == b'=' {
                padding += 1;
                continue;
            }
            if padding > 0 {
                return Err("Invalid base64: data after padding".into());
            }
            let value = BASE64_DECODE[usize::from(byte)];
            if value == INVALID_BASE64 {
                return Err(format!("Invalid base64 character: {:?}", byte as char));
            }
            group[group_len] = value;
            group_len += 1;
            if group_len == 4 {
                let combined = (u32::from(group[0]) << 18)
                    | (u32::from(group[1]) << 12)
                    | (u32::from(group[2]) << 6)
                    | u32::from(group[3]);
                output.push((combined >> 16) as u8);
                output.push((combined >> 8) as u8);
                output.push(combined as u8);
                group_len = 0;
            }
        }
    }

    if !(group_len + padding).is_multiple_of(4) {
        return Err(format!(
            "Invalid base64: length {} (+ {} padding) is not a multiple of 4",
            group_len, padding
        ));
    }

    if group_len >= 2 {
        let combined = (u32::from(group[0]) << 18) | (u32::from(group[1]) << 12);
        output.push((combined >> 16) as u8);
    }
    if group_len == 3 {
        let combined =
            (u32::from(group[0]) << 18) | (u32::from(group[1]) << 12) | (u32::from(group[2]) << 6);
        output.push((combined >> 8) as u8);
    }

    // If there was padding, trim the extra bytes from the last full group.
    if group_len == 0 && padding > 0 {
        let trim: usize = output.len().saturating_sub(padding);
        output.truncate(trim);
    }

    Ok(output)
}

const INVALID_BASE64: u8 = u8::MAX;
const BASE64_DECODE: [u8; 256] = {
    let mut table = [INVALID_BASE64; 256];
    let mut index = 0;
    while index < 26 {
        table[b'A' as usize + index] = index as u8;
        table[b'a' as usize + index] = index as u8 + 26;
        index += 1;
    }
    index = 0;
    while index < 10 {
        table[b'0' as usize + index] = index as u8 + 52;
        index += 1;
    }
    table[b'+' as usize] = 62;
    table[b'/' as usize] = 63;
    table
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hello() {
        // "Hello" = SGVsbG8=
        assert_eq!(base64_decode("SGVsbG8=").unwrap(), b"Hello".to_vec());
    }

    #[test]
    fn test_whitespace_handling() {
        // Simulates how .map files split base64 across lines.
        let input = "SGVs\n  bG8=\n";
        assert_eq!(base64_decode(input).unwrap(), b"Hello".to_vec());
    }

    #[test]
    fn test_invalid_char() {
        assert!(base64_decode("SGVs!G8=").is_err());
    }

    #[test]
    fn test_binary_roundtrip() {
        // Verify decoding of known binary data.
        // [0x00, 0xFF, 0x80] = AP+A
        let result: Vec<u8> = base64_decode("AP+A").unwrap();
        assert_eq!(result, vec![0x00, 0xFF, 0x80]);
    }

    #[test]
    fn chunks_share_one_base64_stream_and_group_state() {
        assert_eq!(
            base64_decode_chunks(&["SG", "Vs", "bG8="]).unwrap(),
            b"Hello"
        );
        assert_eq!(
            base64_decode_chunks(&["SGVs\n", "  bG8=\n"]).unwrap(),
            b"Hello"
        );
    }
}
