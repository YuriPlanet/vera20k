//! Base64 decoder for IsoMapPack5 and other binary sections in RA2 .map files.
//!
//! The ordinary decoder validates padding length and skips ASCII whitespace.
//! Packed terrain and overlays use the game's four-character decoding pipe.
//!
//! ## Dependency rules
//! - Part of util/ — no dependencies on game modules.

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

/// Decode a base64 string to raw bytes.
///
/// Skips any whitespace/newline characters (map files store base64 data across
/// multiple numbered INI lines). Returns an error if invalid characters are found.
pub fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    // Build a flat buffer of 6-bit values, skipping whitespace.
    let mut sextet_buf: Vec<u8> = Vec::with_capacity(input.len());
    let mut padding: usize = 0;

    for ch in input.chars() {
        if ch.is_ascii_whitespace() {
            continue;
        }
        if ch == '=' {
            padding += 1;
            continue;
        }
        if padding > 0 {
            return Err("Invalid base64: data after padding".into());
        }
        let val = sextet(ch).ok_or_else(|| format!("Invalid base64 character: {ch:?}"))?;
        sextet_buf.push(val);
    }

    if !(sextet_buf.len() + padding).is_multiple_of(4) {
        return Err(format!(
            "Invalid base64: length {} (+ {} padding) is not a multiple of 4",
            sextet_buf.len(),
            padding
        ));
    }

    // Each group of 4 sextets → 3 bytes. Final group may produce 1 or 2 bytes.
    let full_groups: usize = sextet_buf.len() / 4;
    let remainder: usize = sextet_buf.len() % 4;
    let mut output: Vec<u8> = Vec::with_capacity(full_groups * 3 + 2);

    for group in sextet_buf.chunks(4) {
        if group.len() == 4 {
            let combined: u32 = (group[0] as u32) << 18
                | (group[1] as u32) << 12
                | (group[2] as u32) << 6
                | (group[3] as u32);
            output.push((combined >> 16) as u8);
            output.push((combined >> 8) as u8);
            output.push(combined as u8);
        } else if group.len() == 3 {
            // 3 sextets → 2 bytes (1 padding char).
            let combined: u32 =
                (group[0] as u32) << 18 | (group[1] as u32) << 12 | (group[2] as u32) << 6;
            output.push((combined >> 16) as u8);
            output.push((combined >> 8) as u8);
        } else if group.len() == 2 {
            // 2 sextets → 1 byte (2 padding chars).
            let combined: u32 = (group[0] as u32) << 18 | (group[1] as u32) << 12;
            output.push((combined >> 16) as u8);
        }
    }

    // If there was padding, trim the extra bytes from the last full group.
    if remainder == 0 && padding > 0 {
        let trim: usize = output.len().saturating_sub(padding);
        output.truncate(trim);
    }

    Ok(output)
}

fn sextet(ch: char) -> Option<u8> {
    match ch {
        'A'..='Z' => Some(ch as u8 - b'A'),
        'a'..='z' => Some(ch as u8 - b'a' + 26),
        '0'..='9' => Some(ch as u8 - b'0' + 52),
        '+' => Some(62),
        '/' => Some(63),
        _ => None,
    }
}

/// Base64Pipe::Put/Flush (0x42DDB0, 0x42DF40) pass four physical input
/// characters at a time to the decoder (0x42FE50), including a final short
/// group. An INI character represents one widened game byte. Within each
/// group '=' ends input and the lookup table skips non-alphabet characters.
pub(crate) fn packed_section_bytes(input: &str) -> impl Iterator<Item = u8> + '_ {
    let mut chars = input.chars();
    std::iter::from_fn(move || {
        let group = [
            Some(chars.next()?),
            chars.next(),
            chars.next(),
            chars.next(),
        ];
        let mut bits = 0u32;
        let mut count = 0;
        for ch in group.into_iter().flatten().take_while(|&ch| ch != '=') {
            if let Some(value) = sextet(ch) {
                bits |= u32::from(value) << (18 - 6 * count);
                count += 1;
            }
        }
        // At 0x42FF14 the first byte is unconditional, then another for
        // three source sextets and another for four.
        let bytes = [(bits >> 16) as u8, (bits >> 8) as u8, bits as u8];
        Some(
            bytes
                .into_iter()
                .take(1 + usize::from(count >= 3) + usize::from(count == 4)),
        )
    })
    .flatten()
}

/// Encode bytes as base64 with the standard alphabet and `=` padding, the
/// form [`base64_decode`] reads.
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let combined = u32::from(group[0]) << 16
            | u32::from(*group.get(1).unwrap_or(&0)) << 8
            | u32::from(*group.get(2).unwrap_or(&0));
        for index in 0..4 {
            if index <= group.len() {
                out.push(ALPHABET[(combined >> (18 - 6 * index) & 0x3F) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(all(test, tests_util))]
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
}
