//! audio.idx / audio.bag parser for RA2/YR sound data.
//!
//! RA2 stores most sound effects (unit voices, EVA announcements, weapon sounds)
//! in `audio.bag` — a flat concatenation of raw audio data — indexed by `audio.idx`.
//! Both RA2 `AUDIO.MIX` and YR `AUDIOMD.MIX` contain `audio.idx` / `audio.bag`.
//! Startup selects sources through AssetManager; this module only parses bytes.
//!
//! ## Format (version 1)
//!
//! **IDX header** (12 bytes):
//! - `+0x00` u32: magic (unused, not validated by engine)
//! - `+0x04` u32: version (must be 1)
//! - `+0x08` u32: entry_count
//!
//! **IDX entry** (32 bytes on disk for v1):
//! - `+0x00` char[16]: name — null-terminated ASCII, max 15 chars
//! - `+0x10` u32: offset — byte position in .bag file
//! - `+0x14` u32: size — byte count in .bag file
//! - `+0x18` u32: sample_rate — Hz (e.g., 22050)
//! - `+0x1C` u32: flags — bit 0=stereo, bit 2=16-bit, bit 3=IMA ADPCM
//!
//! **BAG file**: raw concatenated audio data. No per-entry headers.
//! Each entry's data starts at `offset` and spans `size` bytes.
//!
//! ## Lookup
//! Entries are sorted by name (uppercase) for case-insensitive binary search.
//! The original engine uses `_stricmp` for comparison.
//!
//! ## Dependency rules
//! - Part of assets/ — standalone parser, no game dependencies.
//! - Compressed entries decode through `assets::ima_adpcm`, the crate's single
//!   IMA ADPCM implementation, mirroring the single decoder in `gamemd.exe`.

use crate::util::read_helpers::read_u32_le;

const IDX_HEADER_SIZE: usize = 12;
/// V1 entries are 32 bytes, V2 entries are 36 bytes (extra chunk_size field).
const IDX_ENTRY_SIZE_V1: usize = 32;

#[cfg(test)]
#[path = "audio_bag_native_tests.rs"]
mod native_tests;
const IDX_ENTRY_SIZE_V2: usize = 36;

const FLAG_STEREO: u32 = 0x01;
const FLAG_16BIT: u32 = 0x04;
const FLAG_IMA_ADPCM: u32 = 0x08;

/// Metadata for one sound entry from audio.idx.
#[derive(Debug, Clone)]
pub struct AudioBagEntry {
    /// Sound name (uppercase, max 15 chars).
    pub name: String,
    /// Byte offset into the .bag file.
    pub offset: u32,
    /// Byte count in the .bag file.
    pub size: u32,
    /// Sample rate in Hz (e.g., 22050).
    pub sample_rate: u32,
    /// Raw flags bitfield.
    pub flags: u32,
    /// IMA ADPCM block alignment in bytes (nBlockAlign from WAV fmt).
    /// Zero for non-ADPCM entries or v1 IDX files. When nonzero, the data
    /// is structured as blocks of this size, each starting with a 4-byte
    /// preamble (int16 predictor, u8 step_index, u8 reserved).
    pub chunk_size: u32,
}

impl AudioBagEntry {
    pub fn is_stereo(&self) -> bool {
        self.flags & FLAG_STEREO != 0
    }
    pub fn is_16bit(&self) -> bool {
        self.flags & FLAG_16BIT != 0
    }
    pub fn is_ima_adpcm(&self) -> bool {
        self.flags & FLAG_IMA_ADPCM != 0
    }
    pub fn channels(&self) -> u16 {
        if self.is_stereo() { 2 } else { 1 }
    }
}

/// Decoded audio from a bag entry, ready for conversion to playback format.
pub struct BagAudio {
    /// Signed 16-bit PCM samples (mono or stereo interleaved).
    pub samples_i16: Vec<i16>,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Number of channels (1=mono, 2=stereo).
    pub channels: u16,
}

/// Parsed audio.idx index with the .bag data, providing name-based lookup.
pub struct AudioIndex {
    /// Entries sorted by name (uppercase) for binary search.
    entries: Vec<AudioBagEntry>,
    /// Backing files, retained independently: offsets stay relative to their BAG.
    bag_data: Vec<Vec<u8>>,
    /// Parallel to entries; constructed by parsing or the sole merge owner below.
    entry_bags: Vec<usize>,
    #[cfg(feature = "ares")]
    layout: IndexLayout,
}

#[derive(Clone, Copy)]
enum IndexLayout {
    Stock,
    #[cfg(feature = "ares")]
    Ares,
}

impl AudioIndex {
    /// Parse an audio.idx + audio.bag pair.
    ///
    /// Returns `None` if the idx is malformed or has an unsupported version.
    pub fn from_idx_bag(idx_data: &[u8], bag_data: Vec<u8>) -> Option<Self> {
        Self::parse(idx_data, bag_data, IndexLayout::Stock)
    }

    /// Installed Ares AudioBag::Open1005AA00, selected by Audio_Load4011C0.
    /// See tools/rules_oracle/ares_audio_bags.md and its executed controls.
    #[cfg(feature = "ares")]
    pub(crate) fn from_ares_idx_bag(idx_data: &[u8], bag_data: Vec<u8>) -> Option<Self> {
        Self::parse(idx_data, bag_data, IndexLayout::Ares)
    }

    fn parse(idx_data: &[u8], bag_data: Vec<u8>, layout: IndexLayout) -> Option<Self> {
        if idx_data.len() < IDX_HEADER_SIZE {
            log::warn!("AudioIndex: idx too short ({} bytes)", idx_data.len());
            return None;
        }

        let version = u32::from_le_bytes([idx_data[4], idx_data[5], idx_data[6], idx_data[7]]);
        let entry_size = match layout {
            IndexLayout::Stock => match version {
                1 => IDX_ENTRY_SIZE_V1,
                2 => IDX_ENTRY_SIZE_V2,
                _ => {
                    log::warn!("AudioIndex: unsupported version {version} (expected 1 or 2)");
                    return None;
                }
            },
            // 1005AB3C tests DWORD0, not the version at DWORD1. This installed
            // reader accepts other version tags and rejects ordinary 32-byte
            // GABA/version1 files. Preserve it without weakening stock parsing.
            #[cfg(feature = "ares")]
            IndexLayout::Ares if read_u32_le(idx_data, 0) == 1 => IDX_ENTRY_SIZE_V1,
            #[cfg(feature = "ares")]
            IndexLayout::Ares => IDX_ENTRY_SIZE_V2,
        };

        let entry_count =
            u32::from_le_bytes([idx_data[8], idx_data[9], idx_data[10], idx_data[11]]) as usize;

        let expected_size = entry_count
            .checked_mul(entry_size)?
            .checked_add(IDX_HEADER_SIZE)?;
        if idx_data.len() < expected_size {
            log::warn!(
                "AudioIndex: truncated idx ({} bytes, need {} for {} entries)",
                idx_data.len(),
                expected_size,
                entry_count
            );
            return None;
        }

        let mut entries = Vec::with_capacity(entry_count);
        for i in 0..entry_count {
            let base = IDX_HEADER_SIZE + i * entry_size;
            let name_bytes = &idx_data[base..base + 16];
            let name_end = name_bytes.iter().position(|&b| b == 0).unwrap_or(16);
            let name = String::from_utf8_lossy(&name_bytes[..name_end]).to_ascii_uppercase();

            let offset = read_u32_le(idx_data, base + 16);
            let size = read_u32_le(idx_data, base + 20);
            let sample_rate = read_u32_le(idx_data, base + 24);
            let flags = read_u32_le(idx_data, base + 28);
            // V2 entries have chunk_size at +0x20 (IMA ADPCM block alignment).
            let chunk_size = if entry_size >= IDX_ENTRY_SIZE_V2 {
                read_u32_le(idx_data, base + 32)
            } else {
                0
            };

            entries.push(AudioBagEntry {
                name,
                offset,
                size,
                sample_rate,
                flags,
                chunk_size,
            });
        }

        // Sort by name for binary search (matches original engine behavior).
        entries.sort_by(|a, b| a.name.cmp(&b.name));

        log::info!(
            "AudioIndex: loaded {} entries, bag size {} bytes",
            entries.len(),
            bag_data.len()
        );
        Some(Self {
            entries,
            bag_data: vec![bag_data],
            entry_bags: vec![0; entry_count],
            #[cfg(feature = "ares")]
            layout,
        })
    }

    /// AudioLuggage::Create1005A170: later case-insensitive names replace the
    /// entire entry AND its file. This owns both the index and backing mapping;
    /// callers cannot mutate either independently. No offset rebasing/concatenation.
    #[cfg(feature = "ares")]
    pub(crate) fn merge_ares(bags: impl IntoIterator<Item = Self>) -> Self {
        let mut files = Vec::new();
        let mut winners = std::collections::BTreeMap::new();
        for bag in bags {
            let first_file = files.len();
            files.extend(bag.bag_data);
            for (entry, file) in bag.entries.into_iter().zip(bag.entry_bags) {
                winners.insert(entry.name.clone(), (entry, first_file + file));
            }
        }
        let (entries, entry_bags) = winners.into_values().unzip();
        Self {
            entries,
            entry_bags,
            bag_data: files,
            layout: IndexLayout::Ares,
        }
    }

    /// Look up a sound by name (case-insensitive).
    ///
    /// Stock lookup retains its `.aud` extension conveniences; an Ares catalog
    /// uses the installed original exact case-insensitive FindSampleIndex.
    /// Returns the entry metadata and a slice of the raw audio data from the .bag.
    pub fn get(&self, name: &str) -> Option<(&AudioBagEntry, &[u8])> {
        // Ares keeps original FindSampleIndex4015C0; no extension aliases.
        #[cfg(feature = "ares")]
        if matches!(self.layout, IndexLayout::Ares) {
            return self.get_exact(name);
        }
        // Try exact name first.
        if let Some(result) = self.get_exact(name) {
            return Some(result);
        }
        // Try with .aud extension (entries may include the extension).
        if !name.contains('.')
            && let Some(result) = self.get_exact(&format!("{}.aud", name))
        {
            return Some(result);
        }
        // Try stripping .aud extension (caller may include it but index doesn't).
        if let Some(stem) = name
            .strip_suffix(".aud")
            .or_else(|| name.strip_suffix(".AUD"))
            && let Some(result) = self.get_exact(stem)
        {
            return Some(result);
        }
        None
    }

    /// `AudioIndex::FindSampleIndex @ 0x004015C0`: `bsearch` of the name table
    /// with `_stricmp` (`0x007C8D20`), which `0x004013B1..0x004013BA` sorted
    /// with the same comparator, so a name is found exactly when it equals an
    /// entry case-insensitively. No extension aliases; the bag is not read.
    pub fn find_sample_index(&self, name: &str) -> Option<usize> {
        let upper = name.to_ascii_uppercase();
        self.entries
            .binary_search_by(|e| e.name.as_str().cmp(&upper))
            .ok()
    }

    /// Exact binary search by uppercase name.
    fn get_exact(&self, name: &str) -> Option<(&AudioBagEntry, &[u8])> {
        let idx = self.find_sample_index(name)?;
        let entry = &self.entries[idx];
        let start = entry.offset as usize;
        let end = start.checked_add(entry.size as usize)?;
        let bag = &self.bag_data[self.entry_bags[idx]];
        if end > bag.len() {
            log::warn!(
                "AudioIndex: entry '{}' overflows bag (offset={}, size={}, bag_len={})",
                name,
                entry.offset,
                entry.size,
                bag.len()
            );
            return None;
        }
        Some((entry, &bag[start..end]))
    }

    /// Number of indexed entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// All indexed entries, in index order.
    ///
    /// `names_with_prefix` yields only names; anything reporting on the bag
    /// needs the offsets, rates and flags too.
    pub fn entries(&self) -> &[AudioBagEntry] {
        &self.entries
    }

    /// Return entry names matching a prefix (case-insensitive). For debugging.
    pub fn names_with_prefix(&self, prefix: &str) -> Vec<&str> {
        let upper = prefix.to_ascii_uppercase();
        self.entries
            .iter()
            .filter(|e| e.name.starts_with(&upper))
            .map(|e| e.name.as_str())
            .collect()
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Decode raw audio data from a .bag entry based on its format flags.
///
/// Supports raw PCM (8-bit unsigned, 16-bit signed) and IMA ADPCM with
/// per-block preambles (standard Microsoft IMA ADPCM block format).
pub fn decode_bag_audio(entry: &AudioBagEntry, data: &[u8]) -> Option<BagAudio> {
    if data.is_empty() {
        return None;
    }

    let samples_i16 = if entry.is_ima_adpcm() {
        // `AudioIndex__GetFormat @ 0x00401640` turns flags bit 3 into compression
        // id 1 and passes `chunk_size` (entry +0x20) through as the block stride;
        // `FUN_00409C40 @ 0x00409C40` then wires the one block decoder,
        // `IMA_ADPCM__DecodeBlock @ 0x0040AA70`.
        super::ima_adpcm::decode_blocks(data, entry.channels(), entry.chunk_size)
    } else {
        // Raw PCM, 16-bit signed little-endian or 8-bit unsigned, through the
        // one PCM converter the WAV reader also uses.
        let bits = if entry.is_16bit() { 16 } else { 8 };
        let mut cursor = super::wav_file::WavPcmCursor::pcm(bits)?;
        let mut samples = Vec::with_capacity(data.len());
        while cursor.decode_next(data, &mut samples) {}
        samples
    };

    Some(BagAudio {
        samples_i16,
        sample_rate: entry.sample_rate,
        channels: entry.channels(),
    })
}

/// Index fixtures, shared with client tests through `test-support`.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    /// Build the retail GABA/version2 layout, accepted by stock and Ares.
    pub fn build_idx(entries: &[(&str, u32, u32, u32, u32)]) -> Vec<u8> {
        let mut idx = Vec::new();
        idx.extend_from_slice(&0x41424147u32.to_le_bytes()); // GABA
        idx.extend_from_slice(&2u32.to_le_bytes()); // version
        idx.extend_from_slice(&(entries.len() as u32).to_le_bytes()); // count
        for &(name, offset, size, rate, flags) in entries {
            let mut name_buf = [0u8; 16];
            let name_bytes = name.as_bytes();
            let copy_len = name_bytes.len().min(15);
            name_buf[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
            idx.extend_from_slice(&name_buf);
            idx.extend_from_slice(&offset.to_le_bytes());
            idx.extend_from_slice(&size.to_le_bytes());
            idx.extend_from_slice(&rate.to_le_bytes());
            idx.extend_from_slice(&flags.to_le_bytes());
            idx.extend_from_slice(&0u32.to_le_bytes()); // chunk size
        }
        idx
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::build_idx;
    use super::*;

    #[test]
    fn test_bad_version() {
        let mut idx = build_idx(&[]);
        // Overwrite version to 99 (unsupported — v1 and v2 are valid).
        idx[4..8].copy_from_slice(&99u32.to_le_bytes());
        assert!(AudioIndex::from_idx_bag(&idx, vec![]).is_none());
    }

    #[test]
    fn test_case_insensitive_lookup() {
        let idx = build_idx(&[
            ("igisea", 0, 2, 22050, 0x00), // 8-bit PCM
        ]);
        let bag = vec![128, 192]; // two 8-bit samples
        let index = AudioIndex::from_idx_bag(&idx, bag).expect("should parse");

        assert!(index.get("igisea").is_some());
        assert!(index.get("IGISEA").is_some());
        assert!(index.get("IgiSea").is_some());
        assert!(index.get("notfound").is_none());
    }

    #[test]
    fn test_decode_pcm_16bit() {
        let entry = AudioBagEntry {
            name: "TEST".into(),
            offset: 0,
            size: 4,
            sample_rate: 22050,
            flags: FLAG_16BIT,
            chunk_size: 0,
        };
        let data: &[u8] = &[0x00, 0x40, 0x00, 0xC0]; // 16384, -16384
        let audio = decode_bag_audio(&entry, data).expect("should decode");
        assert_eq!(audio.samples_i16.len(), 2);
        assert_eq!(audio.samples_i16[0], 16384);
        assert_eq!(audio.samples_i16[1], -16384);
        assert_eq!(audio.channels, 1);
    }

    #[test]
    fn test_decode_pcm_8bit() {
        let entry = AudioBagEntry {
            name: "TEST".into(),
            offset: 0,
            size: 3,
            sample_rate: 22050,
            flags: 0, // 8-bit mono
            chunk_size: 0,
        };
        let data: &[u8] = &[128, 255, 0]; // center, max, min
        let audio = decode_bag_audio(&entry, data).expect("should decode");
        assert_eq!(audio.samples_i16.len(), 3);
        assert_eq!(audio.samples_i16[0], 0); // 128 - 128 = 0
        assert_eq!(audio.samples_i16[1], 127 * 256); // 255 - 128 = 127
        assert_eq!(audio.samples_i16[2], -128 * 256); // 0 - 128 = -128
    }

    #[test]
    fn test_bag_overflow_returns_none() {
        let idx = build_idx(&[
            ("overflow", 0, 100, 22050, 0x00), // size 100 but bag only has 2 bytes
        ]);
        let bag = vec![0, 0];
        let index = AudioIndex::from_idx_bag(&idx, bag).expect("should parse");
        assert!(index.get("overflow").is_none());
    }

    // ---------------------------------------------------------------------
    // Retail v2 index bytes.
    //
    // Every retail sound is looked up through a version-2 `audio.idx` with the
    // 36-byte stride; the shared fixture writer also emits that stride. These
    // bytes are copied verbatim from the retail
    // `audio.idx` inside `langmd.mix -> audiomd.mix` (2,285 entries), so the
    // stride, field offsets, flag bits and `chunk_size` are pinned against the
    // shipped file rather than against our own writer.
    // ---------------------------------------------------------------------

    /// The retail 12-byte header: magic `GABA`, version 2, 2,285 entries.
    const RETAIL_IDX_HEADER: [u8; 12] = [
        0x47, 0x41, 0x42, 0x41, 0x02, 0x00, 0x00, 0x00, 0xed, 0x08, 0x00, 0x00,
    ];

    /// Four retail entries, one per (flags, chunk_size) shape that occurs in
    /// AUDIOMD: mono IMA (2,192 entries), stereo IMA (5), mono raw PCM (84),
    /// stereo raw PCM (4).
    const RETAIL_ENTRIES: [[u8; 36]; 4] = [
        // "abirj01a": off 11770646, size 14388, 22050 Hz, flags 12, chunk 512
        [
            0x61, 0x62, 0x69, 0x72, 0x6a, 0x30, 0x31, 0x61, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x16, 0x9b, 0xb3, 0x00, 0x34, 0x38, 0x00, 0x00, 0x22, 0x56, 0x00, 0x00,
            0x0c, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00,
        ],
        // "grexselb": off 7063684, size 41212, flags 13 (stereo IMA), chunk 1024
        [
            0x67, 0x72, 0x65, 0x78, 0x73, 0x65, 0x6c, 0x62, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x84, 0xc8, 0x6b, 0x00, 0xfc, 0xa0, 0x00, 0x00, 0x22, 0x56, 0x00, 0x00,
            0x0d, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00,
        ],
        // "icraloop": off 285960, size 16606, flags 6 (mono raw 16-bit), chunk 0
        [
            0x69, 0x63, 0x72, 0x61, 0x6c, 0x6f, 0x6f, 0x70, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x08, 0x5d, 0x04, 0x00, 0xde, 0x40, 0x00, 0x00, 0x22, 0x56, 0x00, 0x00,
            0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
        // "uscolo1": off 8107566, size 309980, flags 7 (stereo raw 16-bit), chunk 0
        [
            0x75, 0x73, 0x63, 0x6f, 0x6c, 0x6f, 0x31, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x2e, 0xb6, 0x7b, 0x00, 0xdc, 0xba, 0x04, 0x00, 0x22, 0x56, 0x00, 0x00,
            0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ],
    ];

    /// Retail header + entries, with only the entry count rewritten to the
    /// number of entries actually embedded.
    fn retail_idx_slice() -> Vec<u8> {
        let mut idx = RETAIL_IDX_HEADER.to_vec();
        idx[8..12].copy_from_slice(&(RETAIL_ENTRIES.len() as u32).to_le_bytes());
        for entry in &RETAIL_ENTRIES {
            idx.extend_from_slice(entry);
        }
        idx
    }

    #[test]
    fn retail_v2_index_parses_with_the_36_byte_stride() {
        let index = AudioIndex::from_idx_bag(&retail_idx_slice(), Vec::new())
            .expect("retail v2 header must parse");
        assert_eq!(index.len(), RETAIL_ENTRIES.len());

        let by_name = |n: &str| {
            index
                .entries()
                .iter()
                .find(|e| e.name == n)
                .unwrap_or_else(|| panic!("missing {n}"))
        };

        // Names are uppercased on load; the shipped index stores them lowercase.
        let abirj = by_name("ABIRJ01A");
        assert_eq!((abirj.offset, abirj.size), (11_770_646, 14_388));
        assert_eq!(abirj.sample_rate, 22_050);
        assert_eq!(abirj.chunk_size, 512);
        assert!(abirj.is_ima_adpcm() && abirj.is_16bit() && !abirj.is_stereo());
        // 28 whole 512-byte blocks and a 52-byte tail — the shape the block
        // decoder actually sees.
        assert_eq!(abirj.size % abirj.chunk_size, 52);

        let grex = by_name("GREXSELB");
        assert_eq!(grex.chunk_size, 1024, "stereo IMA doubles the stride");
        assert!(grex.is_ima_adpcm() && grex.is_stereo());
        assert_eq!(grex.channels(), 2);

        let icra = by_name("ICRALOOP");
        assert!(!icra.is_ima_adpcm() && icra.is_16bit() && !icra.is_stereo());
        assert_eq!(icra.chunk_size, 0, "raw PCM carries no block stride");

        let usc = by_name("USCOLO1");
        assert!(!usc.is_ima_adpcm() && usc.is_16bit() && usc.is_stereo());
        assert_eq!(usc.chunk_size, 0);

        // Sorted for the native's bsearch over `_stricmp` (0x004013AB / 0x007c8d20).
        let names: Vec<&str> = index.entries().iter().map(|e| e.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
    }

    /// The retail final block of AUDIOMD `ABIRJ01A` (bag offset 11,770,646 +
    /// 28 x 512): 4-byte preamble + 48-byte payload.
    const ABIRJ01A_TAIL_BLOCK: &[u8] = &[
        0xf8, 0xff, 0x01, 0x00, 0x17, 0xbc, 0x33, 0xa1, 0x1b, 0x00, 0x90, 0x3a, 0xb5, 0x2c, 0x92,
        0x10, 0xb2, 0x9c, 0x32, 0xb3, 0x2d, 0x21, 0xcb, 0x79, 0xa1, 0x0a, 0x31, 0xc0, 0x1a, 0x23,
        0xbb, 0x4b, 0xb2, 0x1b, 0x13, 0x11, 0xd9, 0x39, 0x92, 0xa9, 0x30, 0x90, 0x0a, 0x11, 0x11,
        0x1a, 0x91, 0x09, 0x11, 0x09, 0x00, 0x00,
    ];

    #[test]
    fn retail_ima_entry_decodes_through_the_shared_block_decoder() {
        // Entry metadata is the shipped ABIRJ01A row; the data is its real final
        // block, so this walks idx flags -> channels/chunk_size -> block decoder.
        let entry = AudioBagEntry {
            name: "ABIRJ01A".into(),
            offset: 0,
            size: ABIRJ01A_TAIL_BLOCK.len() as u32,
            sample_rate: 22_050,
            flags: 12,
            chunk_size: 512,
        };
        let audio = decode_bag_audio(&entry, ABIRJ01A_TAIL_BLOCK).expect("should decode");
        assert_eq!(audio.channels, 1);
        assert_eq!(audio.sample_rate, 22_050);
        assert_eq!(audio.samples_i16.len(), 97);
        assert_eq!(audio.samples_i16[0], -8, "preamble predictor is sample 0");
        assert_eq!(
            audio.samples_i16,
            crate::assets::ima_adpcm::decode_blocks(ABIRJ01A_TAIL_BLOCK, 1, 512)
        );
    }
}
