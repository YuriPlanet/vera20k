//! Retail corpus inventory and decoder regression baselines.
//!
//! `certify_*` tests compare decoded structure with retail bytes and preserve
//! bounded historical native-reading assertions cited at each check. Running
//! this suite does not establish a new native execution comparison. `ratchet_*`
//! tests compare current Rust decoder output with prior Rust output only.
//!
//! The corpus deliberately includes all disk MIXes, including content outside
//! active YR startup reach. Direct `detect_format` classification and archive
//! index iteration order define the existing committed baseline.

mod decoder_ratchets;
mod manifest;

#[cfg(all(test, tests_asset_tools))]
mod certify_audio;
#[cfg(all(test, tests_asset_tools))]
mod certify_roundtrip;
#[cfg(all(test, tests_asset_tools))]
mod certify_structural;
#[cfg(all(test, tests_asset_tools))]
mod ratchet_tests;
#[cfg(all(test, tests_asset_tools))]
mod tests;

use std::collections::BTreeMap;
use std::path::Path;

use crate::assets::asset_manager::AssetManager;
use crate::assets::audio_bag::{AudioIndex, decode_bag_audio};
use crate::assets::format_sniff::detect_format;
use crate::util::fnv::{FNV1A64_OFFSET_BASIS, fnv1a64_fold_bytes};

use manifest::Manifest;
#[cfg(test)]
use manifest::read_manifest;
pub use manifest::{CandidateReport, export_candidate};

struct CorpusEntry {
    archive: String,
    id: i32,
    #[cfg(test)]
    size: usize,
    format: &'static str,
}

/// Every indexed payload must be readable. Unclassified formats are excluded
/// deliberately; unreadable entries are errors even when their format is unknown.
fn walk_sniffed(am: &AssetManager, mut f: impl FnMut(&CorpusEntry, &[u8])) -> Result<(), String> {
    let mut failures = Vec::new();
    am.visit_archives(|name, archive| {
        if let Err(error) = walk_archive_sniffed(name, archive, &mut f) {
            failures.push(error);
        }
    });
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

fn walk_archive_sniffed(
    name: &str,
    archive: &crate::assets::mix_archive::MixArchive,
    f: &mut impl FnMut(&CorpusEntry, &[u8]),
) -> Result<(), String> {
    for entry in archive.entries() {
        let data = archive.get_by_id(entry.id).ok_or_else(|| {
            format!(
                "{name} {:#010X}: indexed payload is unreadable",
                entry.id as u32
            )
        })?;
        let Some(format) = detect_format(data) else {
            continue;
        };
        f(
            &CorpusEntry {
                archive: name.to_string(),
                id: entry.id,
                #[cfg(test)]
                size: data.len(),
                format,
            },
            data,
        );
    }
    Ok(())
}

type ArchiveInventory = BTreeMap<String, (usize, u64)>;
type FormatCounts = BTreeMap<String, usize>;

fn corpus_inventory(am: &AssetManager) -> Result<(ArchiveInventory, FormatCounts), String> {
    let mut archives = BTreeMap::new();
    let mut format_counts = BTreeMap::new();
    am.visit_archives(|name, archive| {
        let mut h = FNV1A64_OFFSET_BASIS;
        for entry in archive.entries() {
            h = fnv1a64_fold_bytes(h, &(entry.id as u32).to_le_bytes());
            h = fnv1a64_fold_bytes(h, &entry.size.to_le_bytes());
        }
        archives.insert(name.to_string(), (archive.entry_count(), h));
    });
    walk_sniffed(am, |ce, _| {
        *format_counts.entry(ce.format.to_string()).or_default() += 1;
    })?;
    require_formats(format_counts.keys().map(String::as_str))?;
    Ok((archives, format_counts))
}

fn require_formats<'a>(formats: impl Iterator<Item = &'a str>) -> Result<(), String> {
    let formats: Vec<_> = formats.collect();
    let missing: Vec<_> = super::verb_parse_check::COVERED_FORMATS
        .into_iter()
        .filter(|format| !formats.contains(format))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "retail corpus has zero coverage for required formats: {}",
            missing.join(", ")
        ))
    }
}

fn audio_index(am: &AssetManager, mix_name: &str) -> Result<AudioIndex, String> {
    let mix = am
        .archive(mix_name)
        .ok_or_else(|| format!("{mix_name}: archive not loaded"))?;
    let idx = mix
        .get_by_name("audio.idx")
        .ok_or_else(|| format!("{mix_name}: no audio.idx entry"))?;
    let bag = mix
        .get_by_name("audio.bag")
        .ok_or_else(|| format!("{mix_name}: no audio.bag entry"))?;
    let index = AudioIndex::from_idx_bag(idx, bag.to_vec())
        .ok_or_else(|| format!("{mix_name}: audio.idx failed to parse"))?;
    if index.is_empty() {
        return Err(format!("{mix_name}: audio.idx has no entries"));
    }
    Ok(index)
}

/// Both retail audio MIXes are required. Every indexed entry must resolve and decode.
fn audio_bag_total(am: &AssetManager) -> Result<usize, String> {
    let mut failures = Vec::new();
    let mut total = 0;
    for mix_name in ["AUDIOMD.MIX", "AUDIO.MIX"] {
        let index = audio_index(am, mix_name)?;
        total += index.len();
        for name in index.names_with_prefix("") {
            let Some((entry, data)) = index.get(name) else {
                failures.push(format!("{mix_name}: entry '{name}' failed lookup"));
                continue;
            };
            if decode_bag_audio(entry, data).is_none() {
                failures.push(format!(
                    "{mix_name}: entry '{name}' ({} bytes, flags {:#x}) failed to decode",
                    entry.size, entry.flags
                ));
            }
        }
    }
    if failures.is_empty() {
        Ok(total)
    } else {
        Err(format!(
            "audio.bag: {} failures across {total} entries:\n{}",
            failures.len(),
            failures.join("\n")
        ))
    }
}

fn collect_manifest(am: &AssetManager) -> Result<Manifest, String> {
    let (archives, format_counts) = corpus_inventory(am)?;
    Ok(Manifest {
        schema: 1,
        archives,
        format_counts,
        bag_aud: audio_bag_total(am)?,
        decode_rollups: decoder_ratchets::decode_rollups(am)?,
        files: decoder_ratchets::named_file_digests(am)?,
    })
}

fn reject_ambient_write() -> Result<(), String> {
    if std::env::var("RETAIL_GOLDENS_WRITE").is_ok_and(|v| v == "1") {
        return Err("RETAIL_GOLDENS_WRITE=1 is retired. Tests never write baselines. Unset it; use asset corpus-baseline --all-mixes --out <NEW_FILE> to export a candidate for review.".to_string());
    }
    Ok(())
}

#[cfg(test)]
fn required_corpus() -> AssetManager {
    reject_ambient_write().expect("read-only retail validation");
    let (root, _) =
        super::root::resolve_ra2_dir(None).expect("retail corpus requires RA2_DIR or config.toml");
    super::root::open_manager(
        &root,
        true,
        Default::default(),
        &crate::util::archive_names::ArchiveNames::default(),
    )
    .expect("mount the complete retail archive corpus")
}

#[cfg(test)]
fn certify_format(format: &str, mut check: impl FnMut(&CorpusEntry, &[u8]) -> Result<(), String>) {
    let am = required_corpus();
    let mut failures = Vec::new();
    let mut total = 0;
    walk_sniffed(&am, |ce, data| {
        if ce.format != format {
            return;
        }
        total += 1;
        if let Err(msg) = check(ce, data) {
            failures.push(format!(
                "{} {:#010X} ({} bytes): {msg}",
                ce.archive, ce.id as u32, ce.size
            ));
        }
    })
    .expect("read every indexed corpus entry");
    assert!(total > 0, "retail corpus contained no {format} files");
    assert!(
        failures.is_empty(),
        "{format}: {} of {total} retail files violated invariants:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[cfg(test)]
fn xcc_name_map() -> std::collections::HashMap<i32, String> {
    let Ok(db) = crate::assets::xcc_database::XccDatabase::load_from_disk() else {
        return Default::default();
    };
    db.build_hash_dictionary()
        .into_iter()
        .map(|(name, id)| (id, name))
        .collect()
}
