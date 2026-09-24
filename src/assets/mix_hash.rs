//! Filename hashing for MIX archives.
//!
//! MIX archives store 32-bit hash IDs instead of filenames. Two different
//! hash algorithms exist across the C&C game series:
//!
//! ## CRC-32 hash (Tiberian Sun, Red Alert 2 — "new format" MIX files)
//! Standard IEEE CRC-32 computed on the uppercase ASCII filename, BUT with
//! a special padding scheme: the filename is padded to a 4-byte boundary
//! before computing the CRC. The padding bytes encode the residue length
//! and repeat a character from the string. This is why plain CRC-32 only
//! works for filenames whose length is already a multiple of 4.
//!
//! ## Westwood hash (Tiberian Dawn, Red Alert 1 — "old format" MIX files)
//! A custom rolling hash that processes the filename in 4-byte chunks.
//! Used by old-format archives where the first u16 is the file count.
//!
//! ## Dependency rules
//! - Part of assets/ — no dependencies on game modules.

/// IEEE 802.3 CRC-32 polynomial in reflected (bit-reversed) form.
/// Standard polynomial used by Ethernet, ZIP, PNG, etc.
const CRC32_POLYNOMIAL: u32 = 0xEDB88320;

/// Precomputed CRC-32 lookup table (256 entries).
/// Generated at compile time from the IEEE polynomial.
const CRC32_TABLE: [u32; 256] = {
    let mut table: [u32; 256] = [0u32; 256];
    let mut i: usize = 0;
    while i < 256 {
        let mut crc: u32 = i as u32;
        let mut j: usize = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ CRC32_POLYNOMIAL;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

/// Compute the MIX file ID using the Westwood CRC-32 hash (TS/RA2 archives).
///
/// This is standard IEEE CRC-32 but with a special padding scheme applied
/// to the filename before hashing. The filename is:
/// 1. Converted to uppercase ASCII
/// 2. Padded to a 4-byte boundary with special bytes:
///    - First pad byte: residue (length - rounded_down_to_4)
///    - Remaining pad bytes: copy of char at the rounded-down position
/// 3. Fed through standard CRC-32 (init=0xFFFFFFFF, final XOR=0xFFFFFFFF)
///
/// This padding scheme was used by Westwood in TS/RA2.
///
/// ## Why the padding matters
/// Without it, only filenames whose length is a multiple of 4 will match
/// (e.g., "SNOW.MIX" = 8 chars works, but "LOCAL.MIX" = 9 chars does not).
pub fn mix_hash(name: &str) -> i32 {
    let upper: Vec<u8> = name.bytes().map(|b| b.to_ascii_uppercase()).collect();
    crc_engine(&upper) as i32
}

/// `CRCEngine::operator()` @ `0x004A1DE0` over raw bytes: CRC-32 of the
/// Westwood-padded data, no case folding. INI section lookups hash names
/// with it; MIX IDs are this over the uppercased filename.
pub fn crc_engine(data: &[u8]) -> u32 {
    crc32(&westwood_pad(data))
}

/// Apply Westwood's filename padding scheme before CRC-32 computation.
///
/// Pads the input to a 4-byte boundary. If already aligned, no padding.
/// Otherwise appends: [residue_byte, fill_char, fill_char, ...]
/// where residue_byte = length % 4, and fill_char = byte at the
/// last 4-byte-aligned position.
///
/// ## Example: "RULES.INI" (9 bytes)
/// ```text
/// Original:  R  U  L  E  S  .  I  N  I
/// Residue:   9 - 8 = 1
/// Fill char: byte at position 8 = 'I'
/// Padded:    R  U  L  E  S  .  I  N  I  0x01  I  I
/// ```
fn westwood_pad(data: &[u8]) -> Vec<u8> {
    let len: usize = data.len();
    let residue: usize = len % 4;

    if residue == 0 {
        return data.to_vec();
    }

    let padding_count: usize = 4 - residue;
    let rounded_pos: usize = len - residue;
    let fill_char: u8 = data[rounded_pos];

    let mut padded: Vec<u8> = Vec::with_capacity(len + padding_count);
    padded.extend_from_slice(data);

    // First padding byte is the residue value (1, 2, or 3).
    padded.push(residue as u8);

    // Remaining padding bytes are copies of char at the rounded position.
    for _ in 1..padding_count {
        padded.push(fill_char);
    }

    padded
}

/// Compute the MIX file ID using the old Westwood hash (TD/RA1 archives).
///
/// Processes the uppercase filename in 4-byte chunks with bit rotation.
/// Used by old-format archives (Tiberian Dawn, Red Alert 1).
pub fn westwood_hash(name: &str) -> i32 {
    let upper: Vec<u8> = name.bytes().map(|b| b.to_ascii_uppercase()).collect();
    let len: usize = upper.len();
    let mut a: u32 = 0;
    let mut i: usize = 0;

    while i < len {
        let mut buffer: u32 = 0;
        for _j in 0..4 {
            buffer >>= 8;
            if i < len {
                buffer = buffer.wrapping_add((upper[i] as u32) << 24);
                i += 1;
            }
        }
        a = a.rotate_left(1).wrapping_add(buffer);
    }

    a as i32
}

/// Compute IEEE CRC-32 of a byte slice.
///
/// Standard algorithm: init with 0xFFFFFFFF, XOR each byte through
/// the lookup table, final XOR with 0xFFFFFFFF.
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for &byte in data {
        let index: usize = ((crc ^ byte as u32) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC32_TABLE[index];
    }
    crc ^ 0xFFFFFFFF
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32_known_value() {
        // Well-known test vector: CRC-32 of "123456789" = 0xCBF43926.
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
    }

    #[test]
    fn crc_engine_matches_native_execution() {
        // 0x004A1DE0 executed under the Unicorn runner (tools/native_oracle.py)
        // for the two section names the options/RMG readers hash.
        assert_eq!(crc_engine(b"Network"), 0x70CA_A741);
        assert_eq!(crc_engine(b"RandomMap"), 0x1597_B573);
    }

    #[test]
    fn test_mix_hash_case_insensitive() {
        assert_eq!(mix_hash("rules.ini"), mix_hash("RULES.INI"));
        assert_eq!(mix_hash("Local.Mix"), mix_hash("LOCAL.MIX"));
    }

    #[test]
    fn test_mix_hash_different_names() {
        assert_ne!(mix_hash("rules.ini"), mix_hash("art.ini"));
        assert_ne!(mix_hash("local.mix"), mix_hash("conquer.mix"));
    }

    #[test]
    fn test_no_padding_for_multiple_of_4() {
        // Filenames with length divisible by 4 should not be padded.
        // "SNOW.MIX" = 8 chars → no padding → plain CRC-32.
        let plain: Vec<u8> = b"SNOW.MIX".to_vec();
        let padded: Vec<u8> = westwood_pad(&plain);
        assert_eq!(plain, padded, "8-char name should have no padding");
    }

    #[test]
    fn test_padding_for_9_chars() {
        // "RULES.INI" = 9 chars → padded to 12.
        // Residue=1, fill='I' (char at pos 8).
        // Padded: R U L E S . I N I 0x01 I I
        let input: Vec<u8> = b"RULES.INI".to_vec();
        let padded: Vec<u8> = westwood_pad(&input);
        assert_eq!(padded.len(), 12);
        assert_eq!(padded[9], 0x01); // residue byte
        assert_eq!(padded[10], b'I'); // fill char
        assert_eq!(padded[11], b'I'); // fill char
    }

    #[test]
    fn test_padding_for_10_chars() {
        // "CLAT01.TEM" = 10 chars → padded to 12.
        // Residue=2, fill='E' (char at pos 8).
        // Padded: C L A T 0 1 . T E M 0x02 E
        let input: Vec<u8> = b"CLAT01.TEM".to_vec();
        let padded: Vec<u8> = westwood_pad(&input);
        assert_eq!(padded.len(), 12);
        assert_eq!(padded[10], 0x02); // residue byte
        assert_eq!(padded[11], b'E'); // fill char
    }

    #[test]
    fn test_padding_for_11_chars() {
        // "CONQUER.MIX" = 11 chars → padded to 12.
        // Residue=3, fill='M' (char at pos 8).
        // Padded: C O N Q U E R . M I X 0x03
        let input: Vec<u8> = b"CONQUER.MIX".to_vec();
        let padded: Vec<u8> = westwood_pad(&input);
        assert_eq!(padded.len(), 12);
        assert_eq!(padded[11], 0x03); // residue byte (only 1 pad byte needed)
    }

    #[test]
    fn test_westwood_hash_case_insensitive() {
        assert_eq!(westwood_hash("rules.ini"), westwood_hash("RULES.INI"));
    }
}
