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

#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use crate::assets::error::AssetError;
use crate::assets::mix_crypto;
use crate::assets::mix_hash::mix_hash;
use crate::util::read_helpers::{read_u16_le, read_u32_le};

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
    /// The disk file holding these bytes, also for an archive nested in it.
    container_path: Option<Arc<Path>>,
}

#[derive(Clone)]
enum MixBytes {
    Owned(Arc<[u8]>),
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
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

    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
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
            #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
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
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        {
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
        Self::sniff(data).unwrap_or(false)
    }

    /// [`Self::looks_like_mix`]'s answer, or `None` when the bytes end before
    /// a header field.
    fn sniff(data: &[u8]) -> Option<bool> {
        let first_word: u16 = read_u16_le(data, 0)?;

        // The index header: file_count, then body_size.
        let index_header_at = if first_word == 0 {
            NEW_FORMAT_HEADER_SIZE
        } else {
            0
        };
        let file_count: u16 = read_u16_le(data, index_header_at)?;
        let body_size: u32 = read_u32_le(data, index_header_at + 2)?;

        if first_word == 0 && (read_u16_le(data, 2)? & FLAG_ENCRYPTED) != 0 {
            let min_header = NEW_FORMAT_HEADER_SIZE
                + mix_crypto::RSA_KEY_BLOCK_SIZE
                + mix_crypto::BLOWFISH_BLOCK_SIZE;
            return Some(data.len() >= min_header);
        }

        let index_bytes = (file_count as usize).checked_mul(INDEX_ENTRY_SIZE)?;
        let body_offset = (index_header_at + INDEX_HEADER_SIZE).checked_add(index_bytes)?;
        Some(Self::header_bounds_are_sane(data, body_offset, body_size))
    }

    fn from_data(data: MixData) -> Result<Self, AssetError> {
        let bytes = data.as_slice();
        // The first u16 distinguishes old vs new MIX format.
        // New format (RA2/TS): offset 0 is always 0x0000 as a format marker.
        // Old-header layout: offset 0 is file_count (always non-zero).
        let (Some(first_word), Some(second_word)) = (read_u16_le(bytes, 0), read_u16_le(bytes, 2))
        else {
            return Err(AssetError::InvalidMixHeader {
                reason: format!("File too small: {} bytes (need at least 4)", bytes.len()),
            });
        };
        let flags = if first_word == 0 { second_word } else { 0 };
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
            container_path: None,
        })
    }

    /// Load a MIX archive from a file path.
    pub fn load(path: &Path) -> Result<Self, AssetError> {
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        let data = {
            let bytes = crate::util::file_system::read_shared(path)?;
            let len = bytes.len();
            MixData {
                bytes: MixBytes::Owned(bytes),
                start: 0,
                len,
            }
        };
        // WASI has no memory maps (memmap2's stub refuses every map), so the
        // headless wasm determinism run reads each archive whole.
        #[cfg(target_os = "wasi")]
        let data = MixData::from_owned(std::fs::read(path)?);
        #[cfg(not(any(
            all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"),
            target_os = "wasi"
        )))]
        let data = {
            let file = File::open(path)?;
            // SAFETY: memmap2 cannot stop another process from changing the
            // file under the map. Nothing in the game writes an archive it has
            // open; a file changed outside it mid-game is the residual memmap2
            // documents, taken for reads without copies.
            let data = unsafe { memmap2::MmapOptions::new().map(&file)? };
            MixData::from_mapped(data)
        };
        let mut archive = Self::from_data(data)?;
        archive.container_path = Some(Arc::from(path));
        Ok(archive)
    }

    pub(super) fn container_path(&self) -> Option<&Path> {
        self.container_path.as_deref()
    }

    /// Look up a file by name. Returns the raw bytes if found.
    /// Retail provenance: MIX filename resolution — `MixFileSystem__Resolve_First` @ `0x005B4430`.
    /// Retail provenance: Padded filename CRC — `CRCEngine__AddData` @ `0x004A1DE0`.
    /// Header layout affects parsing only; active YR resolves every archive with one CRC ID.
    pub fn get_by_name(&self, name: &str) -> Option<&[u8]> {
        self.get_by_id(mix_hash(name))
    }

    /// `MixFileSystem__Resolve_First` @ `0x005B4430` (`0x005B44CB..0x005B44FB`)
    /// probes the midpoint of a signed-ID sorted index and stops at equality.
    /// The midpoint rule also decides which entry a repeated ID selects.
    fn entry_by_id(&self, id: i32) -> Option<&MixEntry> {
        let mut entries = self.entries.as_slice();
        loop {
            let mid = entries.len() / 2;
            let entry = entries.get(mid)?;
            match entry.id.cmp(&id) {
                std::cmp::Ordering::Less => entries = &entries[mid + 1..],
                std::cmp::Ordering::Greater => entries = &entries[..mid],
                std::cmp::Ordering::Equal => return Some(entry),
            }
        }
    }

    /// Look up a file by its pre-computed hash ID.
    pub fn get_by_id(&self, id: i32) -> Option<&[u8]> {
        let entry = self.entry_by_id(id)?;
        let data = self.data.as_slice();

        let start = self.body_offset.checked_add(entry.offset as usize)?;
        let end = start.checked_add(entry.size as usize)?;

        // Bounds check to prevent panics on corrupt data.
        if end > data.len() {
            return None;
        }

        Some(&data[start..end])
    }

    /// Whether the index lists `id`, readable or not.
    pub fn contains_id(&self, id: i32) -> bool {
        self.entry_by_id(id).is_some()
    }

    /// [`Self::get_by_id`] as a share of the archive storage.
    pub fn get_shared_by_id(&self, id: i32) -> Option<SharedBytes> {
        let entry = self.entry_by_id(id)?;
        self.entry_view(entry).map(SharedBytes)
    }

    /// Parse a child MIX archive directly from an entry without copying its bytes.
    pub fn nested_archive_by_id(&self, id: i32) -> Result<Option<Self>, AssetError> {
        let Some(entry) = self.entry_by_id(id) else {
            return Ok(None);
        };
        let Some(data) = self.entry_view(entry) else {
            return Ok(None);
        };
        let mut archive = Self::from_data(data)?;
        archive.container_path.clone_from(&self.container_path);
        Ok(Some(archive))
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
        // RSA key block starts after the 4-byte new-format header; the
        // encrypted index starts right after the RSA block.
        let encrypted_start: usize = NEW_FORMAT_HEADER_SIZE + mix_crypto::RSA_KEY_BLOCK_SIZE;
        let Some((rsa_block, encrypted)) = data
            .get(NEW_FORMAT_HEADER_SIZE..)
            .and_then(|rest| rest.split_at_checked(mix_crypto::RSA_KEY_BLOCK_SIZE))
        else {
            return Err(AssetError::InvalidMixHeader {
                reason: format!(
                    "File too small for RSA key block: {} bytes (need at least {})",
                    data.len(),
                    encrypted_start
                ),
            });
        };

        // Extract the 56-byte Blowfish key from the RSA-encrypted block.
        let blowfish_key: [u8; 56] = mix_crypto::extract_blowfish_key(rsa_block)?;

        // Decrypt the first 8-byte block to peek at file_count and body_size.
        let Some(mut first_block) = encrypted
            .first_chunk::<{ mix_crypto::BLOWFISH_BLOCK_SIZE }>()
            .copied()
        else {
            return Err(AssetError::InvalidMixHeader {
                reason: "File too small for encrypted index header".to_string(),
            });
        };
        mix_crypto::blowfish_decrypt_ecb(&blowfish_key, &mut first_block)?;

        let [count_0, count_1, size_0, size_1, size_2, size_3, ..] = first_block;
        let file_count = u16::from_le_bytes([count_0, count_1]);
        let body_size = u32::from_le_bytes([size_0, size_1, size_2, size_3]) as usize;

        // Calculate total encrypted index size and decrypt everything.
        let total_index_bytes: usize = INDEX_HEADER_SIZE + file_count as usize * INDEX_ENTRY_SIZE;
        // Round up to Blowfish block boundary (8 bytes).
        let encrypted_size: usize = total_index_bytes.div_ceil(mix_crypto::BLOWFISH_BLOCK_SIZE)
            * mix_crypto::BLOWFISH_BLOCK_SIZE;

        // Copy and decrypt the entire index.
        let Some(encrypted_index) = encrypted.get(..encrypted_size) else {
            return Err(AssetError::InvalidMixHeader {
                reason: format!(
                    "File too small for encrypted index: {} bytes (need {})",
                    data.len(),
                    encrypted_start + encrypted_size
                ),
            });
        };
        let mut decrypted_index: Vec<u8> = encrypted_index.to_vec();
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
        let (Some(file_count), Some(body_size)) = (
            read_u16_le(data, NEW_FORMAT_HEADER_SIZE),
            read_u32_le(data, NEW_FORMAT_HEADER_SIZE + 2),
        ) else {
            return Err(AssetError::InvalidMixHeader {
                reason: "File too small for new-format unencrypted header".to_string(),
            });
        };
        let body_size = body_size as usize;

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
        // body_size at offset 2 (skip file_count which is the first 2 bytes).
        let Some(body_size) = read_u32_le(data, 2) else {
            return Err(AssetError::InvalidMixHeader {
                reason: "File too small for old-header MIX header".to_string(),
            });
        };
        let body_size = body_size as usize;

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
        let Some(table) = data.get(start..).and_then(|index| {
            index
                .as_chunks::<INDEX_ENTRY_SIZE>()
                .0
                .get(..file_count as usize)
        }) else {
            return Err(AssetError::InvalidMixHeader {
                reason: format!(
                    "Index of {} entries at {} extends past data ({})",
                    file_count,
                    start,
                    data.len()
                ),
            });
        };

        Ok(table
            .iter()
            .map(
                |&[
                    id_0,
                    id_1,
                    id_2,
                    id_3,
                    at_0,
                    at_1,
                    at_2,
                    at_3,
                    len_0,
                    len_1,
                    len_2,
                    len_3,
                ]| {
                    MixEntry {
                        id: i32::from_le_bytes([id_0, id_1, id_2, id_3]),
                        offset: u32::from_le_bytes([at_0, at_1, at_2, at_3]),
                        size: u32::from_le_bytes([len_0, len_1, len_2, len_3]),
                    }
                },
            )
            .collect())
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

#[cfg(all(test, tests_assets))]
mod tests {
    use super::*;

    #[test]
    fn encrypted_index_discards_each_rsa_blocks_high_byte() {
        // PKey::Decrypt (0x632870) on little-endian blocks 2 and 1 gives the
        // key for this one-entry index. Block 2 decrypts with a high byte of 9.
        let mut archive = vec![0, 0, 2, 0];
        let mut key_block = [0; 80];
        key_block[0] = 2;
        key_block[40] = 1;
        archive.extend_from_slice(&key_block);
        archive.extend_from_slice(&[
            0x21, 0x20, 0xf1, 0x93, 0x17, 0x54, 0x51, 0xba, 0x43, 0x5f, 0xd8, 0x94, 0xf8, 0xc5,
            0x33, 0xbc, 0xbe, 0x16, 0x3b, 0x8b, 0x09, 0x96, 0x1f, 0x21,
        ]);
        archive.extend_from_slice(b"ok");

        let archive = MixArchive::from_bytes(archive).unwrap();
        assert_eq!(archive.get_by_id(42), Some(b"ok".as_slice()));
    }

    /// An archive whose index lists one id four times. The native resolver
    /// (`MixFileSystem__Resolve_First` @ `0x005B4430`) probes the midpoint of
    /// the sorted index and stops at equality, so the repeated id resolves to
    /// the entry the midpoint lands on, not the first or the last.
    #[test]
    fn a_repeated_id_resolves_to_the_midpoint_the_native_probe_finds() {
        let mut entries = vec![MixEntry {
            id: 7,
            offset: 6,
            size: 2,
        }];
        entries.extend((0..4).map(|index| MixEntry {
            id: 42,
            offset: index * 2,
            size: 2,
        }));
        entries.sort_by_key(|entry| entry.id);
        let archive = MixArchive {
            entries,
            body_offset: 0,
            body_size: 8,
            flags: 0,
            data: MixData::from_owned(b"aabbccdd".to_vec()),
            container_path: None,
        };
        // Five entries: the probe lands on index 2, the third of the four.
        assert_eq!(archive.get_by_id(42), Some(b"bb".as_slice()));
        assert_eq!(archive.get_by_id(7), Some(b"dd".as_slice()));
        assert!(archive.contains_id(42));
        assert!(!archive.contains_id(43));
    }
}
