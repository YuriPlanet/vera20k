//! Reader for RA2 .mix archive files.
//!
//! MIX files are Westwood's archive format — like ZIP but without compression.
//! They store game assets (sprites, palettes, sounds, INI files, and even
//! other nested .mix files).
//!
//! ## Format variants
//!
//! ### New format (Tiberian Sun, Red Alert 2):
//! ```text
//! Offset 0: u16 = 0x0000 (new format marker — always zero)
//! Offset 2: u16 = flags  (bit 0 = checksum, bit 1 = encrypted)
//! Offset 4: if encrypted → 80-byte RSA key block, then encrypted index
//!           if not encrypted → file index header
//! ```
//!
//! ### Old-header layout (also used by YR theme archives):
//! ```text
//! Offset 0: u16 = file_count (non-zero — this IS the count, no flags)
//! Offset 2: u32 = body_size
//! Offset 6: file index entries
//! ```
//!
//! ### File index header (6 bytes, inside encrypted block if encrypted):
//! - u16: file_count
//! - u32: body_size
//!
//! ### File index entries (12 bytes each):
//! - i32: id (CRC-32 hash of filename)
//! - u32: offset (from start of body section)
//! - u32: size
//!
//! ## Encryption
//! RA2 MIX files typically have flags = 0x0003 (encrypted + checksum).
//! Only the file index is encrypted — the body data is plaintext.
//! See mix_crypto.rs for the RSA + Blowfish decryption details.
//!
//! ## Dependency rules
//! - Part of assets/ — no dependencies on game modules.
//! - Uses mix_crypto for decryption and mix_hash for filename lookup.

use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use crate::assets::error::AssetError;
use crate::assets::mix_crypto;
use crate::assets::mix_hash::mix_hash;
use crate::util::read_helpers::{read_i32_le, read_u16_le, read_u32_le};

/// MIX flags bit: file index is Blowfish-encrypted (with RSA key block).
const FLAG_ENCRYPTED: u16 = 0x0002;

/// Size of the new-format header: 2-byte marker (0x0000) + 2-byte flags.
/// The RSA key block or file index starts after this.
const NEW_FORMAT_HEADER_SIZE: usize = 4;

/// Size of each file index entry in bytes: id(4) + offset(4) + size(4).
const INDEX_ENTRY_SIZE: usize = 12;

/// Size of the file index sub-header in bytes: file_count(2) + body_size(4).
const INDEX_HEADER_SIZE: usize = 6;

/// A single entry in the MIX file index.
///
/// Each entry maps a hashed filename (i32 id) to a byte range
/// within the MIX body section. The id is computed from the
/// original filename using CRC-32 (see mix_hash.rs).
#[derive(Debug, Clone)]
pub struct MixEntry {
    /// CRC-32 hash of the original filename (cast to i32).
    pub id: i32,
    /// Byte offset of this file's data within the body section.
    pub offset: u32,
    /// Size of this file's data in bytes.
    pub size: u32,
}

/// A parsed MIX archive, ready for file extraction.
///
/// Holds the entire MIX file data in memory and provides
/// methods to look up and extract files by name or hash ID.
#[derive(Clone)]
pub struct MixArchive {
    /// The file index entries, sorted by id for binary search lookup.
    entries: Vec<MixEntry>,
    /// Byte offset where the body section starts in the raw data.
    body_offset: usize,
    /// The raw archive bytes. Top-level archives can stay memory-mapped;
    /// nested archives can share a sub-view into parent archive storage.
    data: MixData,
}

#[derive(Clone)]
enum MixBytes {
    Owned(Arc<[u8]>),
    Mapped(Arc<memmap2::Mmap>),
}

#[derive(Clone)]
struct MixData {
    bytes: MixBytes,
    start: usize,
    len: usize,
}

impl MixData {
    fn from_owned(data: Vec<u8>) -> Self {
        let bytes: Arc<[u8]> = data.into();
        let len = bytes.len();
        Self {
            bytes: MixBytes::Owned(bytes),
            start: 0,
            len,
        }
    }

    fn from_mapped(map: memmap2::Mmap) -> Self {
        let bytes = Arc::new(map);
        let len = bytes.len();
        Self {
            bytes: MixBytes::Mapped(bytes),
            start: 0,
            len,
        }
    }

    fn as_slice(&self) -> &[u8] {
        let bytes = match &self.bytes {
            MixBytes::Owned(data) => &data[..],
            MixBytes::Mapped(map) => &map[..],
        };
        &bytes[self.start..self.start + self.len]
    }

    fn subrange(&self, start: usize, end: usize) -> Option<Self> {
        if start > end || end > self.len {
            return None;
        }
        Some(Self {
            bytes: self.bytes.clone(),
            start: self.start + start,
            len: end - start,
        })
    }

    fn len(&self) -> usize {
        self.len
    }
}

impl MixArchive {
    /// Parse a MIX archive from raw bytes.
    ///
    /// Detects old vs new format by checking the first u16:
    /// - 0x0000 → new format (RA2/TS): real flags at offset 2
    /// - Non-zero → old-header layout: that value IS the file_count
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, AssetError> {
        Self::from_data(MixData::from_owned(data))
    }

    /// Cheap format sniff used before cloning candidate nested MIX payloads.
    pub fn looks_like_mix(data: &[u8]) -> bool {
        if data.len() < INDEX_HEADER_SIZE {
            return false;
        }

        let first_word: u16 = read_u16_le(data, 0);

        if first_word == 0 {
            if data.len() < NEW_FORMAT_HEADER_SIZE + INDEX_HEADER_SIZE {
                return false;
            }

            let flags: u16 = read_u16_le(data, 2);
            if (flags & FLAG_ENCRYPTED) != 0 {
                let min_header = NEW_FORMAT_HEADER_SIZE
                    + mix_crypto::RSA_KEY_BLOCK_SIZE
                    + mix_crypto::BLOWFISH_BLOCK_SIZE;
                return data.len() >= min_header;
            }

            let file_count: u16 = read_u16_le(data, NEW_FORMAT_HEADER_SIZE);
            let body_size: u32 = read_u32_le(data, NEW_FORMAT_HEADER_SIZE + 2);
            let Some(index_bytes) = (file_count as usize).checked_mul(INDEX_ENTRY_SIZE) else {
                return false;
            };
            let Some(body_offset) =
                (NEW_FORMAT_HEADER_SIZE + INDEX_HEADER_SIZE).checked_add(index_bytes)
            else {
                return false;
            };
            return Self::header_bounds_are_sane(data, body_offset, body_size);
        }

        let file_count: u16 = first_word;
        let body_size: u32 = read_u32_le(data, 2);
        let Some(index_bytes) = (file_count as usize).checked_mul(INDEX_ENTRY_SIZE) else {
            return false;
        };
        let Some(body_offset) = INDEX_HEADER_SIZE.checked_add(index_bytes) else {
            return false;
        };
        Self::header_bounds_are_sane(data, body_offset, body_size)
    }

    fn from_data(data: MixData) -> Result<Self, AssetError> {
        let bytes = data.as_slice();
        if bytes.len() < 4 {
            return Err(AssetError::InvalidMixHeader {
                reason: format!("File too small: {} bytes (need at least 4)", bytes.len()),
            });
        }

        // The first u16 distinguishes old vs new MIX format.
        // New format (RA2/TS): offset 0 is always 0x0000 as a format marker.
        // Old-header layout: offset 0 is file_count (always non-zero).
        let first_word: u16 = read_u16_le(bytes, 0);
        let (mut entries, body_offset) = if first_word == 0 {
            // New format: actual flags are at offset 2.
            let flags: u16 = read_u16_le(bytes, 2);
            let is_encrypted: bool = (flags & FLAG_ENCRYPTED) != 0;

            if is_encrypted {
                Self::parse_encrypted_new_format(bytes)?
            } else {
                Self::parse_unencrypted_new_format(bytes)?
            }
        } else {
            // Old-header layout: first_word IS the file_count. No flags, no encryption.
            Self::parse_old_format(bytes, first_word)?
        };
        entries.sort_by_key(|e| e.id);

        Ok(Self {
            entries,
            body_offset,
            data,
        })
    }

    /// Load a MIX archive from a file path.
    pub fn load(path: &Path) -> Result<Self, AssetError> {
        let file = File::open(path)?;
        let data = unsafe { memmap2::MmapOptions::new().map(&file)? };
        Self::from_data(MixData::from_mapped(data))
    }

    /// Look up a file by name. Returns the raw bytes if found.
    /// Retail provenance: MIX filename resolution — `MixFileSystem__Resolve_First` @ `0x005B4430`.
    /// Retail provenance: Padded filename CRC — `CRCEngine__AddData` @ `0x004A1DE0`.
    /// Header layout affects parsing only; active YR resolves every archive with one CRC ID.
    pub fn get_by_name(&self, name: &str) -> Option<&[u8]> {
        self.get_by_id(mix_hash(name))
    }

    /// Look up a file by its pre-computed hash ID.
    ///
    /// Uses binary search on the sorted index for O(log n) lookup.
    pub fn get_by_id(&self, id: i32) -> Option<&[u8]> {
        let index: usize = self.entries.binary_search_by_key(&id, |e| e.id).ok()?;
        let entry: &MixEntry = &self.entries[index];
        let data = self.data.as_slice();

        let start: usize = self.body_offset + entry.offset as usize;
        let end: usize = start + entry.size as usize;

        // Bounds check to prevent panics on corrupt data.
        if end > data.len() {
            return None;
        }

        Some(&data[start..end])
    }

    /// Parse a child MIX archive directly from an entry without copying its bytes.
    pub fn nested_archive_by_id(&self, id: i32) -> Result<Option<Self>, AssetError> {
        let index: usize = match self.entries.binary_search_by_key(&id, |e| e.id) {
            Ok(index) => index,
            Err(_) => return Ok(None),
        };
        let Some(data) = self.entry_view(&self.entries[index]) else {
            return Ok(None);
        };
        Self::from_data(data).map(Some)
    }

    /// Number of files in this archive.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Get all entries (for debugging and enumeration).
    pub fn entries(&self) -> &[MixEntry] {
        &self.entries
    }

    /// Size of the raw archive in bytes.
    pub fn byte_len(&self) -> usize {
        self.data.len()
    }

    /// Parse a new-format encrypted MIX file (RA2 typical: flags & 0x0002).
    ///
    /// Layout: [marker:2][flags:2][RSA block:80][encrypted index...][body...]
    /// The RSA block starts at offset 4 (after the 2-byte marker + 2-byte flags).
    fn parse_encrypted_new_format(data: &[u8]) -> Result<(Vec<MixEntry>, usize), AssetError> {
        // RSA key block starts after the 4-byte new-format header.
        let rsa_start: usize = NEW_FORMAT_HEADER_SIZE;
        let rsa_end: usize = rsa_start + mix_crypto::RSA_KEY_BLOCK_SIZE;

        if data.len() < rsa_end {
            return Err(AssetError::InvalidMixHeader {
                reason: format!(
                    "File too small for RSA key block: {} bytes (need at least {})",
                    data.len(),
                    rsa_end
                ),
            });
        }

        // Extract the 56-byte Blowfish key from the RSA-encrypted block.
        let blowfish_key: [u8; 56] = mix_crypto::extract_blowfish_key(&data[rsa_start..rsa_end])?;

        // The encrypted index starts right after the RSA block.
        let encrypted_start: usize = rsa_end;

        // Decrypt the first 8-byte block to peek at file_count and body_size.
        if data.len() < encrypted_start + mix_crypto::BLOWFISH_BLOCK_SIZE {
            return Err(AssetError::InvalidMixHeader {
                reason: "File too small for encrypted index header".to_string(),
            });
        }

        let mut first_block: [u8; 8] = [0u8; 8];
        first_block.copy_from_slice(
            &data[encrypted_start..encrypted_start + mix_crypto::BLOWFISH_BLOCK_SIZE],
        );
        mix_crypto::blowfish_decrypt_ecb(&blowfish_key, &mut first_block)?;

        let file_count: u16 = read_u16_le(&first_block, 0);
        let _body_size: u32 = read_u32_le(&first_block, 2);

        // Calculate total encrypted index size and decrypt everything.
        let total_index_bytes: usize = INDEX_HEADER_SIZE + file_count as usize * INDEX_ENTRY_SIZE;
        // Round up to Blowfish block boundary (8 bytes).
        let encrypted_size: usize = total_index_bytes.div_ceil(mix_crypto::BLOWFISH_BLOCK_SIZE)
            * mix_crypto::BLOWFISH_BLOCK_SIZE;

        if data.len() < encrypted_start + encrypted_size {
            return Err(AssetError::InvalidMixHeader {
                reason: format!(
                    "File too small for encrypted index: {} bytes (need {})",
                    data.len(),
                    encrypted_start + encrypted_size
                ),
            });
        }

        // Copy and decrypt the entire index.
        let mut decrypted_index: Vec<u8> =
            data[encrypted_start..encrypted_start + encrypted_size].to_vec();
        mix_crypto::blowfish_decrypt_ecb(&blowfish_key, &mut decrypted_index)?;

        // Parse entries from the decrypted index buffer.
        let entries: Vec<MixEntry> =
            Self::parse_entries(&decrypted_index, INDEX_HEADER_SIZE, file_count)?;

        // Body data starts right after the encrypted index.
        let body_offset: usize = encrypted_start + encrypted_size;

        Ok((entries, body_offset))
    }

    /// Parse a new-format unencrypted MIX file (flags has no encryption bit).
    ///
    /// Layout: [marker:2][flags:2][file_count:2][body_size:4][entries...][body...]
    /// The index header starts at offset 4.
    fn parse_unencrypted_new_format(data: &[u8]) -> Result<(Vec<MixEntry>, usize), AssetError> {
        if data.len() < NEW_FORMAT_HEADER_SIZE + INDEX_HEADER_SIZE {
            return Err(AssetError::InvalidMixHeader {
                reason: "File too small for new-format unencrypted header".to_string(),
            });
        }

        let file_count: u16 = read_u16_le(data, NEW_FORMAT_HEADER_SIZE);
        let _body_size: u32 = read_u32_le(data, NEW_FORMAT_HEADER_SIZE + 2);

        let index_start: usize = NEW_FORMAT_HEADER_SIZE + INDEX_HEADER_SIZE;
        let index_end: usize = index_start + file_count as usize * INDEX_ENTRY_SIZE;

        if data.len() < index_end {
            return Err(AssetError::InvalidMixHeader {
                reason: format!(
                    "File too small for {} index entries: need {} bytes",
                    file_count, index_end
                ),
            });
        }

        let entries: Vec<MixEntry> = Self::parse_entries(data, index_start, file_count)?;
        let body_offset: usize = index_end;

        Ok((entries, body_offset))
    }

    /// Parse an old-header MIX file (no format marker).
    ///
    /// Layout: [file_count:2][body_size:4][entries...][body...]
    /// The first u16 at offset 0 is already the file_count (passed in).
    fn parse_old_format(
        data: &[u8],
        file_count: u16,
    ) -> Result<(Vec<MixEntry>, usize), AssetError> {
        if data.len() < INDEX_HEADER_SIZE {
            return Err(AssetError::InvalidMixHeader {
                reason: "File too small for old-header MIX header".to_string(),
            });
        }

        // body_size at offset 2 (skip file_count which is the first 2 bytes).
        let _body_size: u32 = read_u32_le(data, 2);

        let index_start: usize = INDEX_HEADER_SIZE;
        let index_end: usize = index_start + file_count as usize * INDEX_ENTRY_SIZE;

        if data.len() < index_end {
            return Err(AssetError::InvalidMixHeader {
                reason: format!(
                    "File too small for {} old-header index entries: need {} bytes",
                    file_count, index_end
                ),
            });
        }

        let entries: Vec<MixEntry> = Self::parse_entries(data, index_start, file_count)?;
        let body_offset: usize = index_end;

        Ok((entries, body_offset))
    }

    /// Parse file index entries from a buffer starting at the given offset.
    fn parse_entries(
        data: &[u8],
        start: usize,
        file_count: u16,
    ) -> Result<Vec<MixEntry>, AssetError> {
        let mut entries: Vec<MixEntry> = Vec::with_capacity(file_count as usize);

        for i in 0..file_count as usize {
            let offset: usize = start + i * INDEX_ENTRY_SIZE;

            if offset + INDEX_ENTRY_SIZE > data.len() {
                return Err(AssetError::InvalidMixHeader {
                    reason: format!(
                        "Index entry {} extends past data ({} > {})",
                        i,
                        offset + INDEX_ENTRY_SIZE,
                        data.len()
                    ),
                });
            }

            entries.push(MixEntry {
                id: read_i32_le(data, offset),
                offset: read_u32_le(data, offset + 4),
                size: read_u32_le(data, offset + 8),
            });
        }

        Ok(entries)
    }

    fn header_bounds_are_sane(data: &[u8], body_offset: usize, body_size: u32) -> bool {
        if body_offset > data.len() {
            return false;
        }
        body_size as usize <= data.len().saturating_sub(body_offset)
    }

    fn entry_view(&self, entry: &MixEntry) -> Option<MixData> {
        let start: usize = self.body_offset + entry.offset as usize;
        let end: usize = start + entry.size as usize;
        if end > self.data.len() {
            return None;
        }
        self.data.subrange(start, end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::mix_hash::westwood_hash;

    /// Build a minimal new-format unencrypted MIX file with one entry.
    ///
    /// Layout: [marker:0x0000][flags:0x0000][file_count:1][body_size:5]
    ///         [index entry: 12 bytes][body: "hello"]
    fn make_test_mix_new_format() -> Vec<u8> {
        let mut data: Vec<u8> = Vec::new();

        // New-format header: marker=0x0000, flags=0x0000 (no encryption)
        data.extend_from_slice(&0u16.to_le_bytes()); // marker
        data.extend_from_slice(&0u16.to_le_bytes()); // flags

        // Index header: file_count=1, body_size=5
        data.extend_from_slice(&1u16.to_le_bytes()); // file_count
        data.extend_from_slice(&5u32.to_le_bytes()); // body_size

        // Index entry: id=hash("test.txt"), offset=0, size=5
        let test_id: i32 = mix_hash("test.txt");
        data.extend_from_slice(&test_id.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes()); // offset
        data.extend_from_slice(&5u32.to_le_bytes()); // size

        // Body data
        data.extend_from_slice(b"hello");

        data
    }

    /// Build a minimal old-header MIX file (like thememd.mix).
    ///
    /// Layout: [file_count:1][body_size:5][index entry: 12 bytes][body: "world"]
    fn make_test_mix_old_header() -> Vec<u8> {
        let mut data: Vec<u8> = Vec::new();

        // Old-header layout: file_count directly at offset 0 (non-zero).
        data.extend_from_slice(&1u16.to_le_bytes()); // file_count=1
        data.extend_from_slice(&5u32.to_le_bytes()); // body_size=5

        // Index entry
        let test_id: i32 = 0x0CFB_D6C3;
        data.extend_from_slice(&test_id.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes()); // offset
        data.extend_from_slice(&5u32.to_le_bytes()); // size

        // Body data
        data.extend_from_slice(b"world");

        data
    }

    fn make_test_mix_with_entry(name: &str, body: &[u8]) -> Vec<u8> {
        let mut data: Vec<u8> = Vec::new();
        let entry_id: i32 = mix_hash(name);

        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&(body.len() as u32).to_le_bytes());
        data.extend_from_slice(&entry_id.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&(body.len() as u32).to_le_bytes());
        data.extend_from_slice(body);

        data
    }

    #[test]
    fn test_parse_new_format_unencrypted() {
        let data: Vec<u8> = make_test_mix_new_format();
        let archive: MixArchive =
            MixArchive::from_bytes(data).expect("Should parse new-format MIX");
        assert_eq!(archive.entry_count(), 1);
    }

    #[test]
    fn parses_old_header_layout() {
        let data: Vec<u8> = make_test_mix_old_header();
        let archive: MixArchive =
            MixArchive::from_bytes(data).expect("Should parse old-header MIX");
        assert_eq!(archive.entry_count(), 1);
    }

    #[test]
    fn get_by_name_uses_one_crc_id_without_rolling_fallback() {
        let data: Vec<u8> = make_test_mix_new_format();
        let archive: MixArchive = MixArchive::from_bytes(data).expect("Should parse");

        let content: &[u8] = archive
            .get_by_name("test.txt")
            .expect("Should find test.txt");
        assert_eq!(content, b"hello");

        let mut rolling_only = make_test_mix_new_format();
        let crc_id = mix_hash("test.txt");
        let rolling_id = westwood_hash("test.txt");
        assert_ne!(crc_id, rolling_id);
        let index_start = NEW_FORMAT_HEADER_SIZE + INDEX_HEADER_SIZE;
        rolling_only[index_start..index_start + 4].copy_from_slice(&rolling_id.to_le_bytes());
        let rolling_only = MixArchive::from_bytes(rolling_only).expect("Should parse");
        assert!(rolling_only.get_by_name("test.txt").is_none());
    }

    #[test]
    fn old_header_get_by_name_uses_crc_id() {
        assert_eq!(mix_hash("BRAINfre.wav"), 0x0CFB_D6C3);
        let data: Vec<u8> = make_test_mix_old_header();
        let archive: MixArchive = MixArchive::from_bytes(data).expect("Should parse");

        let content: &[u8] = archive
            .get_by_name("brainfre.wav")
            .expect("Should find BRAINfre.wav");
        assert_eq!(content, b"world");
    }

    #[test]
    fn test_get_by_name_case_insensitive() {
        let data: Vec<u8> = make_test_mix_new_format();
        let archive: MixArchive = MixArchive::from_bytes(data).expect("Should parse");

        assert!(archive.get_by_name("TEST.TXT").is_some());
        assert!(archive.get_by_name("Test.Txt").is_some());
    }

    #[test]
    fn test_get_nonexistent_file() {
        let data: Vec<u8> = make_test_mix_new_format();
        let archive: MixArchive = MixArchive::from_bytes(data).expect("Should parse");

        assert!(archive.get_by_name("nonexistent.dat").is_none());
    }

    #[test]
    fn test_looks_like_mix_accepts_valid_headers() {
        assert!(MixArchive::looks_like_mix(&make_test_mix_new_format()));
        assert!(MixArchive::looks_like_mix(&make_test_mix_old_header()));
    }

    #[test]
    fn test_looks_like_mix_rejects_non_mix_data() {
        assert!(!MixArchive::looks_like_mix(b"not a mix archive"));
    }

    #[test]
    fn test_nested_archive_by_id_parses_child_without_copying_interface() {
        let child = make_test_mix_with_entry("child.txt", b"nested");
        let parent = MixArchive::from_bytes(make_test_mix_with_entry("child.mix", &child))
            .expect("parent mix should parse");
        let child_id = mix_hash("child.mix");

        let nested = parent
            .nested_archive_by_id(child_id)
            .expect("nested parse should succeed")
            .expect("child entry should exist");

        assert_eq!(
            nested
                .get_by_name("child.txt")
                .expect("child asset should exist"),
            b"nested"
        );
    }

    #[test]
    fn test_reject_too_small() {
        let data: Vec<u8> = vec![0; 2]; // Way too small
        assert!(MixArchive::from_bytes(data).is_err());
    }
}
