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
const FLAG_CHECKSUM: u16 = 0x0001;
const CHECKSUM_SIZE: usize = 20;

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
    body_size: usize,
    flags: u16,
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

/// A file's bytes holding a share of their storage instead of a borrow of the
/// archive, so a reader on another thread can keep reading them (a theme song
/// streamed by the audio device) without a copy.
#[derive(Clone)]
pub struct SharedBytes(MixData);

impl From<Arc<[u8]>> for SharedBytes {
    fn from(bytes: Arc<[u8]>) -> Self {
        let len = bytes.len();
        Self(MixData {
            bytes: MixBytes::Owned(bytes),
            start: 0,
            len,
        })
    }
}

impl SharedBytes {
    /// Fault a mapped file's pages in on a short-lived thread, so a reader
    /// streaming it (a theme song on the audio device) doesn't wait on the
    /// disk mid-play. `StreamPlayer__PlayFile @ 0x00407B60` likewise buffers
    /// seconds of file ahead of its decoder. Owned bytes are already in memory.
    pub fn read_ahead(&self) {
        if !matches!(self.0.bytes, MixBytes::Mapped(_)) {
            return;
        }
        let bytes = self.clone();
        // A failed spawn only loses the warm-up; the reader still faults pages
        // in itself.
        let _ = std::thread::Builder::new()
            .name("asset-read-ahead".into())
            .spawn(move || {
                let touched = bytes.chunks(4096).fold(0u8, |sum, page| sum ^ page[0]);
                std::hint::black_box(touched);
            });
    }
}

impl std::ops::Deref for SharedBytes {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.0.as_slice()
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
        let flags = if first_word == 0 {
            read_u16_le(bytes, 2)
        } else {
            0
        };
        let (mut entries, body_offset, body_size) = if first_word == 0 {
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
            body_size,
            flags,
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

        let start = self.body_offset.checked_add(entry.offset as usize)?;
        let end = start.checked_add(entry.size as usize)?;

        // Bounds check to prevent panics on corrupt data.
        if end > data.len() {
            return None;
        }

        Some(&data[start..end])
    }

    /// [`Self::get_by_id`] as a share of the archive storage.
    pub fn get_shared_by_id(&self, id: i32) -> Option<SharedBytes> {
        let index: usize = self.entries.binary_search_by_key(&id, |e| e.id).ok()?;
        self.entry_view(&self.entries[index]).map(SharedBytes)
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

    /// Validate a completely accounted-for container for an untrusted consumer.
    /// Ordinary retail asset loading keeps its permissive index lookup. A
    /// network map additionally forbids aliases, overlapping entries and
    /// hidden body/trailer data. The caller owns allowed padding byte types.
    pub(crate) fn validate_complete_layout(
        &self,
        padding_byte_allowed: impl Fn(u8) -> bool,
    ) -> Result<(), AssetError> {
        let invalid = |reason: &str| AssetError::InvalidMixHeader {
            reason: reason.to_owned(),
        };
        if self.flags & !(FLAG_CHECKSUM | FLAG_ENCRYPTED) != 0 {
            return Err(invalid("Unsupported transfer MIX flags"));
        }
        let data = self.data.as_slice();
        let body_end = self
            .body_offset
            .checked_add(self.body_size)
            .filter(|&end| end <= data.len())
            .ok_or_else(|| invalid("Invalid transfer MIX body length"))?;
        let checksum_size = if self.flags & FLAG_CHECKSUM != 0 {
            CHECKSUM_SIZE
        } else {
            0
        };
        if data.len().checked_sub(body_end) != Some(checksum_size) {
            return Err(invalid("Unreferenced transfer MIX trailer"));
        }
        let body = &data[self.body_offset..body_end];
        // The flagged retail trailer is SHA-1 of the complete physical body,
        // including alignment bytes (Hills.mmx, Dustbowl.mmx, RiverRam.yro).
        if checksum_size != 0 && crate::util::sha1::sha1_bytes(body).as_slice() != &data[body_end..]
        {
            return Err(invalid("Transfer MIX checksum mismatch"));
        }
        if self.entries.windows(2).any(|pair| pair[0].id == pair[1].id) {
            return Err(invalid("Duplicate transfer MIX file IDs"));
        }
        let mut ranges: Vec<_> = self.entries.iter().collect();
        ranges.sort_by_key(|entry| entry.offset);
        let padding_is_valid = |start: usize, end: usize| {
            // Named retail map wrappers align entries and body end to 16
            // bytes. Their <16-byte padding contains old plaintext residues,
            // not just zeroes; arbitrary gaps remain forbidden.
            start == end
                || (start.checked_next_multiple_of(16) == Some(end)
                    && body[start..end]
                        .iter()
                        .all(|&byte| padding_byte_allowed(byte)))
        };
        let mut cursor = 0usize;
        for entry in ranges {
            let start = entry.offset as usize;
            let end = start
                .checked_add(entry.size as usize)
                .filter(|&end| end <= body.len())
                .ok_or_else(|| invalid("Transfer MIX entry exceeds its body"))?;
            if entry.size == 0 || start < cursor || !padding_is_valid(cursor, start) {
                return Err(invalid("Overlapping or unreferenced transfer MIX data"));
            }
            cursor = end;
        }
        if !padding_is_valid(cursor, body.len()) {
            return Err(invalid("Unreferenced transfer MIX body tail"));
        }
        Ok(())
    }

    /// Parse a new-format encrypted MIX file (RA2 typical: flags & 0x0002).
    ///
    /// Layout: [marker:2][flags:2][RSA block:80][encrypted index...][body...]
    /// The RSA block starts at offset 4 (after the 2-byte marker + 2-byte flags).
    fn parse_encrypted_new_format(
        data: &[u8],
    ) -> Result<(Vec<MixEntry>, usize, usize), AssetError> {
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
        let body_size = read_u32_le(&first_block, 2) as usize;

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

        Ok((entries, body_offset, body_size))
    }

    /// Parse a new-format unencrypted MIX file (flags has no encryption bit).
    ///
    /// Layout: [marker:2][flags:2][file_count:2][body_size:4][entries...][body...]
    /// The index header starts at offset 4.
    fn parse_unencrypted_new_format(
        data: &[u8],
    ) -> Result<(Vec<MixEntry>, usize, usize), AssetError> {
        if data.len() < NEW_FORMAT_HEADER_SIZE + INDEX_HEADER_SIZE {
            return Err(AssetError::InvalidMixHeader {
                reason: "File too small for new-format unencrypted header".to_string(),
            });
        }

        let file_count: u16 = read_u16_le(data, NEW_FORMAT_HEADER_SIZE);
        let body_size = read_u32_le(data, NEW_FORMAT_HEADER_SIZE + 2) as usize;

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

        Ok((entries, body_offset, body_size))
    }

    /// Parse an old-header MIX file (no format marker).
    ///
    /// Layout: [file_count:2][body_size:4][entries...][body...]
    /// The first u16 at offset 0 is already the file_count (passed in).
    fn parse_old_format(
        data: &[u8],
        file_count: u16,
    ) -> Result<(Vec<MixEntry>, usize, usize), AssetError> {
        if data.len() < INDEX_HEADER_SIZE {
            return Err(AssetError::InvalidMixHeader {
                reason: "File too small for old-header MIX header".to_string(),
            });
        }

        // body_size at offset 2 (skip file_count which is the first 2 bytes).
        let body_size = read_u32_le(data, 2) as usize;

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

        Ok((entries, body_offset, body_size))
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
        let start = self.body_offset.checked_add(entry.offset as usize)?;
        let end = start.checked_add(entry.size as usize)?;
        if end > self.data.len() {
            return None;
        }
        self.data.subrange(start, end)
    }
}
