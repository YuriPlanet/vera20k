//! `asset sound <NAME>` and `asset bag-ls` — the audio bag, headlessly.
//!
//! Production uses the one startup-selected `audio.idx`/`audio.bag` pair.
//! Each file follows the shared resolver, including independent loose overrides.
//! Explicit `--bag` requests inspect a named archive or loose diagnostic pair.
//! See [process audio owner](../../tools/audio_catalog_owner.md) for source/lifetime evidence.
//!
//! `bag-ls` never decodes — it exists to answer "does this name exist" over
//! thousands of entries, and decoding them would cost seconds per page for
//! numbers nobody asked for. `sound` decodes one entry, and a decode failure is
//! a warning rather than an error, because the index header (rate, channels,
//! codec, size) is exact and useful even when the payload is not.
//!
//! ## Dependency rules
//! - Part of `asset_tools/`: depends on `assets/` (archives, bag index, decode)
//!   and the sibling `locate` / `report` modules.
//! - Nothing from `sim/`, `render/`, `ui/`, `audio/`, or `net/`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::asset_tools::report::{ErrorReport, SoundEntry, SoundReport};
use crate::assets::asset_manager::{
    AssetManager, AssetResolutionRef, AudioIndexSources, LoadedAudioIndex, load_audio_index_pair,
};
use crate::assets::audio_bag::{AudioBagEntry, AudioIndex, BagAudio, decode_bag_audio};
use crate::assets::mix_hash::mix_hash;

/// Names used inside explicit diagnostic bag archives.
const BAG_IDX_ENTRY: &str = "audio.idx";
const BAG_DATA_ENTRY: &str = "audio.bag";

/// Extensions an agent might spell when naming a bag: the archive, or either
/// half of the pair. All three reduce to the same stem.
const BAG_STRIPPED_EXTENSIONS: [&str; 3] = [".MIX", ".IDX", ".BAG"];

/// Reported as the bag identity when nothing opened, so the field is never an
/// empty string that reads like a missing value.
const NO_BAG_IDENTITY: &str = "none";

/// Appended to the failure text whenever no bag opened. Names the flag rather
/// than describing the problem twice.
const NO_BAG_HINT: &str = "AUDIOMD.MIX and AUDIO.MIX carry audio.idx/audio.bag; pass `--bag audiomd` \
                           or `--bag audio` to name one, and `asset archives` lists what is \
                           actually mounted";

/// Default page size for `bag-ls`. Retail's two bags hold thousands of entries
/// between them, so an unpaged dump answers nothing.
pub const DEFAULT_LIMIT: usize = 100;

/// Output root when the caller does not name one. Matches the rest of the tool:
/// under `target/`, which is gitignored, so nothing written here is committable.
const DEFAULT_OUT_ROOT: &str = "target/asset";

/// Subdirectory under the output root that this verb owns.
const SOUND_SUBDIR: &str = "sound";

/// Filename stem used when an entry name sanitises to nothing.
const FALLBACK_FILE_NAME: &str = "sound";

/// Characters of the queried name offered back as a `--prefix` suggestion.
const PREFIX_HINT_CHARS: usize = 3;

// --- Canonical 16-bit PCM RIFF/WAVE header. There is no wav crate here, so the
// 44 bytes are written by hand; the constants are named so the layout below
// reads as the spec rather than as magic numbers. ---

/// `RIFF` + size + `WAVE` + a 16-byte `fmt ` chunk + the `data` chunk header.
const WAV_HEADER_SIZE: usize = 44;
/// Bytes of the RIFF chunk that precede the sample data, i.e. everything after
/// the 8-byte `RIFF` header up to and including the `data` size field.
const WAV_RIFF_PRELUDE: u32 = 36;
/// Payload size of a PCM `fmt ` chunk.
const WAV_FMT_CHUNK_SIZE: u32 = 16;
/// `WAVE_FORMAT_PCM`.
const WAV_FORMAT_PCM: u16 = 1;
const WAV_BITS_PER_SAMPLE: u16 = 16;
const WAV_BYTES_PER_SAMPLE: usize = 2;
/// RIFF sizes are u32. A decode larger than this cannot be described by the
/// header at all, so no file is written rather than one that lies about itself.
const MAX_WAV_DATA_BYTES: usize = (u32::MAX - WAV_RIFF_PRELUDE) as usize;

const MILLIS_PER_SECOND: u64 = 1000;

/// Options for `asset sound` and `asset bag-ls`.
#[derive(Debug, Clone)]
pub struct SoundOptions {
    /// Explicit diagnostic bag pair. None uses the production startup pair.
    pub bag: Option<String>,
    /// `bag-ls` only: case-insensitive name prefix.
    pub prefix: Option<String>,
    /// `bag-ls` only: page size.
    pub limit: usize,
    /// `bag-ls` only: entries skipped before the page.
    pub offset: usize,
    /// Decode and write a .wav next to the other outputs.
    pub wav: bool,
    /// Output root. The .wav lands in `<out>/sound/`.
    pub out: PathBuf,
}

impl Default for SoundOptions {
    fn default() -> Self {
        Self {
            bag: None,
            prefix: None,
            limit: DEFAULT_LIMIT,
            offset: 0,
            wav: false,
            out: PathBuf::from(DEFAULT_OUT_ROOT),
        }
    }
}

/// One opened `.idx`/`.bag` pair plus the provenance string for the report.
struct OpenBag {
    index: AudioIndex,
    /// How this pair was opened, e.g. `AUDIOMD.MIX -> audio.idx/audio.bag`.
    identity: String,
    sources: AudioIndexSources,
}

/// `asset sound <NAME>` — one bag entry's header, decoded length, and
/// optionally a .wav.
///
/// Default lookup uses the production index. `--bag` explicitly overrides it.
pub fn sound(
    asset_manager: &AssetManager,
    name: &str,
    opts: &SoundOptions,
) -> Result<SoundReport, ErrorReport> {
    let (bag, mut warnings) = open_bag(asset_manager, opts);
    let Some(bag) = bag else {
        return Err(ErrorReport {
            error: format!("no audio bag could be opened, so {name} cannot be looked up"),
            hint: Some(open_failure_hint(&warnings)),
        });
    };
    let Some((entry, data)) = bag.index.get(name) else {
        return Err(ErrorReport {
            error: format!("no bag entry named {name}"),
            hint: Some(format!(
                "`asset bag-ls --prefix {}` lists neighbouring names; use --bag for an explicit diagnostic override",
                prefix_hint(name)
            )),
        });
    };
    let entry_count = bag.index.len();
    let mut row = metadata_row(entry);
    match decode_bag_audio(entry, data) {
        Some(audio) => {
            row.decoded_samples = Some(audio.samples_i16.len());
            row.duration_ms =
                duration_ms(audio.samples_i16.len(), audio.channels, audio.sample_rate);
            if row.duration_ms.is_none() {
                warnings.push(format!(
                    "{} declares a sample rate of {} Hz, so its duration is unknown",
                    entry.name, audio.sample_rate
                ));
            }
            if opts.wav {
                match write_wav(&opts.out, &entry.name, &audio) {
                    Ok(path) => row.wav = Some(path),
                    Err(message) => warnings.push(message),
                }
            }
        }
        None => {
            warnings.push(format!(
                "{} ({} bytes, flags {:#04x}) could not be decoded; the index header above is \
                 still exact",
                entry.name, entry.size, entry.flags
            ));
            if opts.wav {
                warnings.push(format!(
                    "no .wav written for {} because the decode produced no samples",
                    entry.name
                ));
            }
        }
    }

    Ok(SoundReport {
        bag: bag.identity,
        sources: Some(bag.sources),
        entry_count,
        matched: 1,
        shown: 1,
        entries: vec![row],
        warnings,
    })
}

/// `asset bag-ls` — a paged, prefix-filtered listing of the bag index.
///
/// Header fields only, from the startup index or explicit diagnostic override.
pub fn bag_ls(
    asset_manager: &AssetManager,
    opts: &SoundOptions,
) -> Result<SoundReport, ErrorReport> {
    let (bag, mut warnings) = open_bag(asset_manager, opts);
    let Some(bag) = bag else {
        warnings.push(format!(
            "no audio bag could be opened, so nothing is listed — {NO_BAG_HINT}"
        ));
        return Ok(SoundReport {
            bag: NO_BAG_IDENTITY.to_string(),
            sources: None,
            entry_count: 0,
            matched: 0,
            shown: 0,
            entries: Vec::new(),
            warnings,
        });
    };

    let (reachable, hidden) = dedupe_by_name(bag.index.entries().iter());
    if hidden > 0 {
        warnings.push(format!(
            "{hidden} duplicate index name(s) omitted from this listing"
        ));
    }

    let prefix_upper = opts.prefix.as_ref().map(|text| text.to_ascii_uppercase());
    let matched = filter_by_prefix(&reachable, prefix_upper.as_deref());
    if matched.is_empty() && prefix_upper.is_some() {
        warnings.push(format!(
            "no entry starts with \"{}\"; run `asset bag-ls` without --prefix to see what the \
             bag holds",
            opts.prefix.as_deref().unwrap_or_default()
        ));
    }
    if opts.limit == 0 {
        warnings.push("--limit 0 shows no rows; pass a positive page size".to_string());
    } else if opts.offset >= matched.len() && !matched.is_empty() {
        warnings.push(format!(
            "--offset {} skips past all {} matching entries",
            opts.offset,
            matched.len()
        ));
    }

    let rows = page(&matched, opts.offset, opts.limit);
    Ok(SoundReport {
        bag: bag.identity,
        sources: Some(bag.sources),
        entry_count: reachable.len(),
        matched: matched.len(),
        shown: rows.len(),
        entries: rows.into_iter().map(metadata_row).collect(),
        warnings,
    })
}

/// Open the production pair, or one explicitly requested diagnostic override.
fn open_bag(asset_manager: &AssetManager, opts: &SoundOptions) -> (Option<OpenBag>, Vec<String>) {
    let mut warnings = Vec::new();
    let Some(requested) = opts.bag.as_deref() else {
        return match asset_manager.load_audio_index() {
            Ok(Some(loaded)) => (Some(opened_pair(loaded)), warnings),
            Ok(None) => (
                None,
                vec!["startup audio.idx/audio.bag pair is unavailable".to_owned()],
            ),
            Err(error) => (None, vec![error.to_string()]),
        };
    };
    let stem = stem_of(requested);
    if stem.is_empty() {
        return (
            None,
            vec![format!("--bag {requested:?} has no name part to open")],
        );
    }
    let archive_name = format!("{stem}.MIX");
    if let Some((source_archive, archive)) = asset_manager.archive_with_source(&archive_name) {
        match (
            archive.get_by_name(BAG_IDX_ENTRY),
            archive.get_by_name(BAG_DATA_ENTRY),
        ) {
            (Some(idx), Some(bag)) => {
                let idx = AssetResolutionRef {
                    bytes: idx,
                    source_archive,
                    entry_id: mix_hash(BAG_IDX_ENTRY),
                };
                let bag = AssetResolutionRef {
                    bytes: bag,
                    source_archive,
                    entry_id: mix_hash(BAG_DATA_ENTRY),
                };
                match load_audio_index_pair(idx, bag) {
                    Ok(loaded) => return (Some(opened_pair(loaded)), warnings),
                    Err(error) => warnings.push(error.to_string()),
                }
            }
            _ => warnings.push(format!(
                "{archive_name} has no complete audio.idx/audio.bag pair"
            )),
        }
    } else {
        warnings.push(format!("{archive_name} is not mounted"));
    }
    // Explicit overrides may inspect loose/custom pairs, including catalog-only
    // sources. This browsing policy never participates in default game lookup.
    let idx_name = format!("{}.idx", stem.to_ascii_lowercase());
    let bag_name = format!("{}.bag", stem.to_ascii_lowercase());
    let (Some(idx), Some(bag)) = (
        crate::asset_tools::locate::locate(asset_manager, &idx_name),
        crate::asset_tools::locate::locate(asset_manager, &bag_name),
    ) else {
        return (None, warnings);
    };
    warnings.extend(idx.catalog_warning());
    warnings.extend(bag.catalog_warning());
    warnings.push(format!(
        "explicit diagnostic pair {idx_name}/{bag_name}; not the production startup pair"
    ));
    let result = load_audio_index_pair(
        AssetResolutionRef {
            bytes: idx.bytes,
            source_archive: &idx.source_archive,
            entry_id: idx.entry_id,
        },
        AssetResolutionRef {
            bytes: bag.bytes,
            source_archive: &bag.source_archive,
            entry_id: bag.entry_id,
        },
    );
    match result {
        Ok(loaded) => (Some(opened_pair(loaded)), warnings),
        Err(error) => {
            warnings.push(error.to_string());
            (None, warnings)
        }
    }
}

fn opened_pair(loaded: LoadedAudioIndex) -> OpenBag {
    let identity = format!(
        "IDX from {}; BAG from {}",
        loaded.sources.idx.source_archive, loaded.sources.bag.source_archive
    );
    OpenBag {
        index: loaded.index,
        sources: loaded.sources,
        identity,
    }
}

/// Hint text for a total open failure: the specific reasons, then the flag.
fn open_failure_hint(warnings: &[String]) -> String {
    if warnings.is_empty() {
        NO_BAG_HINT.to_string()
    } else {
        format!("{} — {NO_BAG_HINT}", warnings.join("; "))
    }
}

/// Reduce `audiomd`, `audiomd.idx`, `audiomd.bag` and `AUDIOMD.MIX` — every
/// spelling an agent reaches for — to the one uppercase stem.
fn stem_of(word: &str) -> String {
    let base = word
        .trim()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_uppercase();
    for extension in BAG_STRIPPED_EXTENSIONS {
        if let Some(stem) = base.strip_suffix(extension) {
            return stem.to_string();
        }
    }
    base
}

/// Keep the first entry for each name and count the rest.
///
/// Duplicate names in one index occupy one listing row.
fn dedupe_by_name<'a>(
    entries: impl Iterator<Item = &'a AudioBagEntry>,
) -> (Vec<&'a AudioBagEntry>, usize) {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut kept: Vec<&AudioBagEntry> = Vec::new();
    let mut hidden = 0usize;
    for entry in entries {
        if seen.insert(entry.name.as_str()) {
            kept.push(entry);
        } else {
            hidden += 1;
        }
    }
    (kept, hidden)
}

/// Case-insensitive, anchored at the start of the name.
fn filter_by_prefix<'a>(
    entries: &[&'a AudioBagEntry],
    prefix_upper: Option<&str>,
) -> Vec<&'a AudioBagEntry> {
    let Some(prefix) = prefix_upper else {
        return entries.to_vec();
    };
    entries
        .iter()
        .copied()
        .filter(|entry| has_prefix(&entry.name, prefix))
        .collect()
}

/// Byte-wise so a name carrying a lossy replacement character cannot panic on a
/// char boundary, and so no allocation happens per row.
fn has_prefix(name: &str, prefix_upper: &str) -> bool {
    let name = name.as_bytes();
    let prefix = prefix_upper.as_bytes();
    name.len() >= prefix.len() && name[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn page<'a>(entries: &[&'a AudioBagEntry], offset: usize, limit: usize) -> Vec<&'a AudioBagEntry> {
    entries.iter().copied().skip(offset).take(limit).collect()
}

/// Index-header fields only. `bag-ls` never fills the decoded fields.
fn metadata_row(entry: &AudioBagEntry) -> SoundEntry {
    SoundEntry {
        name: entry.name.clone(),
        offset: entry.offset,
        size: entry.size,
        sample_rate: entry.sample_rate,
        channels: entry.channels(),
        is_16bit: entry.is_16bit(),
        is_ima_adpcm: entry.is_ima_adpcm(),
        chunk_size: entry.chunk_size,
        decoded_samples: None,
        duration_ms: None,
        wav: None,
    }
}

/// Playback length from the interleaved sample count.
///
/// `samples` counts every channel's samples, so it is divided by the channel
/// count before the rate. `None` when the header declares a zero rate — a
/// duration invented from a divide-by-zero guard would be worse than absent.
fn duration_ms(sample_count: usize, channels: u16, sample_rate: u32) -> Option<u64> {
    if sample_rate == 0 {
        return None;
    }
    let frames = sample_count as u64 / u64::from(channels.max(1));
    Some(frames * MILLIS_PER_SECOND / u64::from(sample_rate))
}

/// Write `<out>/sound/<name>.wav`, returning its path or a ready-made warning.
fn write_wav(out: &Path, name: &str, audio: &BagAudio) -> Result<String, String> {
    if audio.sample_rate == 0 {
        return Err(format!(
            "no .wav written for {name}: the index declares a sample rate of 0 Hz, which no \
             player can interpret"
        ));
    }
    let Some(bytes) = wav_bytes(&audio.samples_i16, audio.channels, audio.sample_rate) else {
        return Err(format!(
            "no .wav written for {name}: {} decoded samples at {} Hz cannot be described by a \
             RIFF header",
            audio.samples_i16.len(),
            audio.sample_rate
        ));
    };

    let dir = sound_dir(out);
    std::fs::create_dir_all(&dir).map_err(|err| {
        format!(
            "could not create {}: {err} — pass a writable `--out` root",
            dir.display()
        )
    })?;
    let path = dir.join(format!("{}.wav", sanitise_name(name)));
    std::fs::write(&path, bytes).map_err(|err| {
        format!(
            "could not write {}: {err} — pass a writable `--out` root",
            path.display()
        )
    })?;
    Ok(path.display().to_string())
}

/// The canonical 44-byte header followed by little-endian interleaved samples.
///
/// `None` when the payload or the derived byte rate overflows a RIFF field,
/// which only a malformed index can produce.
fn wav_bytes(samples: &[i16], channels: u16, sample_rate: u32) -> Option<Vec<u8>> {
    let channels = channels.max(1);
    let data_len = samples.len().checked_mul(WAV_BYTES_PER_SAMPLE)?;
    if data_len > MAX_WAV_DATA_BYTES {
        return None;
    }
    let block_align = channels.checked_mul(WAV_BITS_PER_SAMPLE / 8)?;
    let byte_rate = sample_rate.checked_mul(u32::from(block_align))?;
    let data_len_u32 = data_len as u32;

    let mut out = Vec::with_capacity(WAV_HEADER_SIZE + data_len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(WAV_RIFF_PRELUDE + data_len_u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&WAV_FMT_CHUNK_SIZE.to_le_bytes());
    out.extend_from_slice(&WAV_FORMAT_PCM.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&WAV_BITS_PER_SAMPLE.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len_u32.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    Some(out)
}

/// `<out>/sound/`, absolutised so a reported path does not depend on the
/// reader's working directory.
fn sound_dir(out: &Path) -> PathBuf {
    let root = std::path::absolute(out).unwrap_or_else(|_| out.to_path_buf());
    root.join(SOUND_SUBDIR)
}

/// Replace every character outside `[A-Za-z0-9._-]`, matching the render verb's
/// rule so both verbs' outputs are safe filename components everywhere.
fn sanitise_name(name: &str) -> String {
    let sanitised: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect();
    if sanitised.is_empty() {
        FALLBACK_FILE_NAME.to_string()
    } else {
        sanitised
    }
}

/// The leading characters of a missed name, offered back as `--prefix`.
fn prefix_hint(name: &str) -> String {
    let hint: String = name
        .chars()
        .take(PREFIX_HINT_CHARS)
        .collect::<String>()
        .to_ascii_uppercase();
    if hint.is_empty() {
        FALLBACK_FILE_NAME.to_ascii_uppercase()
    } else {
        hint
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, offset: u32, flags: u32) -> AudioBagEntry {
        AudioBagEntry {
            name: name.to_string(),
            offset,
            size: 64,
            sample_rate: 22050,
            flags,
            chunk_size: 0,
        }
    }

    #[test]
    fn mono_wav_header_is_the_exact_44_byte_riff_layout() {
        // 4 samples * 2 bytes = 8 bytes of data, 22050 Hz mono.
        let wav = wav_bytes(&[0, 1, -1, 32767], 1, 22050).expect("fits a RIFF header");
        assert_eq!(wav.len(), WAV_HEADER_SIZE + 8);
        assert_eq!(
            &wav[..WAV_HEADER_SIZE],
            &[
                b'R', b'I', b'F', b'F', // RIFF
                0x2C, 0x00, 0x00, 0x00, // 36 + 8 = 44
                b'W', b'A', b'V', b'E', // WAVE
                b'f', b'm', b't', b' ', // fmt
                0x10, 0x00, 0x00, 0x00, // PCM fmt chunk is 16 bytes
                0x01, 0x00, // WAVE_FORMAT_PCM
                0x01, 0x00, // 1 channel
                0x22, 0x56, 0x00, 0x00, // 22050 Hz
                0x44, 0xAC, 0x00, 0x00, // 22050 * 1 * 2 = 44100 byte/s
                0x02, 0x00, // block align 2
                0x10, 0x00, // 16 bits per sample
                b'd', b'a', b't', b'a', // data
                0x08, 0x00, 0x00, 0x00, // 8 bytes
            ][..]
        );
    }

    #[test]
    fn a_zero_channel_count_is_written_as_mono_rather_than_dividing_by_zero() {
        let wav = wav_bytes(&[0, 0], 0, 22050).expect("fits a RIFF header");
        assert_eq!(&wav[22..24], &[0x01, 0x00][..]);
    }

    #[test]
    fn an_absurd_sample_rate_produces_no_file_instead_of_overflowing() {
        assert_eq!(wav_bytes(&[0, 0], 2, u32::MAX), None);
    }

    #[test]
    fn duration_divides_by_channels_before_the_rate() {
        // 44100 mono samples at 22050 Hz is two seconds.
        assert_eq!(duration_ms(44_100, 1, 22050), Some(2000));
        // The same interleaved count in stereo is half as long.
        assert_eq!(duration_ms(44_100, 2, 22050), Some(1000));
        // Sub-millisecond clips floor to zero rather than rounding up.
        assert_eq!(duration_ms(10, 1, 22050), Some(0));
        // A zero channel count is treated as mono, never as a divisor.
        assert_eq!(duration_ms(22_050, 0, 22050), Some(1000));
    }

    #[test]
    fn a_zero_sample_rate_has_no_duration_rather_than_a_made_up_one() {
        assert_eq!(duration_ms(44_100, 1, 0), None);
    }

    #[test]
    fn prefix_filtering_is_case_insensitive_and_anchored() {
        let entries = [
            entry("IGISEA", 0, 0),
            entry("IGIDIE", 64, 0),
            entry("CEVA048", 128, 0),
        ];
        let borrowed: Vec<&AudioBagEntry> = entries.iter().collect();

        let all = filter_by_prefix(&borrowed, None);
        assert_eq!(all.len(), 3);

        let igi = filter_by_prefix(&borrowed, Some("IGI"));
        assert_eq!(igi.len(), 2);
        assert_eq!(igi[0].name, "IGISEA");

        // The caller's casing must not matter; bag names are stored uppercase.
        assert_eq!(filter_by_prefix(&borrowed, Some("igi")).len(), 2);
        // Anchored: a substring match would wrongly return CEVA048 here.
        assert!(filter_by_prefix(&borrowed, Some("EVA")).is_empty());
        // A prefix longer than the name cannot match, and must not panic.
        assert!(filter_by_prefix(&borrowed, Some("IGISEAXXXX")).is_empty());
    }

    #[test]
    fn paging_skips_then_caps_and_survives_an_offset_past_the_end() {
        let entries: Vec<AudioBagEntry> = (0..5)
            .map(|i| entry(&format!("SND{i}"), i * 64, 0))
            .collect();
        let borrowed: Vec<&AudioBagEntry> = entries.iter().collect();

        let first = page(&borrowed, 0, 2);
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].name, "SND0");

        let second = page(&borrowed, 2, 2);
        assert_eq!(second.len(), 2);
        assert_eq!(second[0].name, "SND2");

        // A short final page, and an offset past the end, both report themselves
        // by row count rather than erroring.
        assert_eq!(page(&borrowed, 4, 2).len(), 1);
        assert!(page(&borrowed, 99, 2).is_empty());
        assert!(page(&borrowed, 0, 0).is_empty());
    }

    #[test]
    fn duplicate_index_names_are_listed_once() {
        let entries = [entry("CEVA048", 0, 0), entry("CEVA048", 900, 0)];
        let (kept, hidden) = dedupe_by_name(entries.iter());
        assert_eq!(hidden, 1);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].offset, 0);
    }

    #[test]
    fn every_spelling_of_a_bag_reduces_to_one_stem() {
        for word in [
            "audiomd",
            "AUDIOMD",
            "audiomd.idx",
            "audiomd.bag",
            "AUDIOMD.MIX",
            " audiomd ",
        ] {
            assert_eq!(stem_of(word), "AUDIOMD", "spelling {word}");
        }
        // An empty or extension-only value names nothing to open.
        assert!(stem_of("").is_empty());
        assert!(stem_of(".mix").is_empty());
    }

    #[test]
    fn metadata_row_reads_the_flag_bits_into_named_fields() {
        // Bit 0 stereo, bit 2 16-bit, bit 3 IMA ADPCM.
        let row = metadata_row(&entry("TEST", 0, 0x0D));
        assert_eq!(row.channels, 2);
        assert!(row.is_16bit);
        assert!(row.is_ima_adpcm);
        // bag-ls never decodes, so these stay empty.
        assert!(row.decoded_samples.is_none());
        assert!(row.duration_ms.is_none());
        assert!(row.wav.is_none());
    }

    #[test]
    fn audio_archive_fallback_is_whole_pair_and_diagnostics_can_override_it() {
        let dir = crate::assets::asset_manager::test_support::audio_test_install("audio-fallback");
        let md = crate::assets::audio_bag::test_support::build_idx(&[("md_only", 0, 2, 22050, 4)]);
        let base =
            crate::assets::audio_bag::test_support::build_idx(&[("base_only", 0, 2, 22050, 4)]);
        crate::assets::asset_manager::test_support::write_audio_archive(
            &dir,
            "audiomd.mix",
            &md,
            &[1, 2],
        );
        let base_archive = crate::map::source::test_support::make_new_format_mix_bytes(&[
            ("audio.idx", &base),
            ("audio.bag", &[3, 4]),
        ]);
        dir.write_mix("ra2.mix", "audio.mix", &base_archive);
        std::fs::write(dir.path().join("custom.idx"), &base).unwrap();
        std::fs::write(dir.path().join("custom.bag"), [3, 4]).unwrap();
        let manager = AssetManager::new(
            dir.path(),
            crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
        )
        .unwrap();
        let loaded = manager.load_audio_index().unwrap().unwrap();
        assert!(loaded.index.get("md_only").is_some());
        assert!(loaded.index.get("base_only").is_none());
        let explicit = bag_ls(
            &manager,
            &SoundOptions {
                bag: Some("audio".to_owned()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(explicit.entry_count, 1);
        assert!(explicit.entries[0].name.eq_ignore_ascii_case("base_only"));
        assert_eq!(
            explicit.sources.unwrap().idx.source_archive,
            "ra2.mix -> audio.mix"
        );
        let custom = bag_ls(
            &manager,
            &SoundOptions {
                bag: Some("custom".to_owned()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(custom.entry_count, 1);
        assert_eq!(
            custom.sources.unwrap().bag.source_sha256,
            crate::util::sha256::sha256_hex(&[3, 4])
        );
        // Release the archive mappings before removing the YR pair; Windows
        // filesystems can refuse to delete a MIX while it is still mapped.
        drop(manager);
        std::fs::remove_file(dir.path().join("audiomd.mix")).unwrap();
        let fallback = AssetManager::new(
            dir.path(),
            crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
        )
        .unwrap();
        let loaded = fallback.load_audio_index().unwrap().unwrap();
        assert!(loaded.index.get("base_only").is_some());
        assert_eq!(loaded.sources.idx.source_archive, "ra2.mix -> audio.mix");
    }
}
