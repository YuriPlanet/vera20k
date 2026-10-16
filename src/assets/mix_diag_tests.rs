//! Integration tests for MIX archive loading against real RA2 files.
//! Tests skip gracefully if RA2 assets aren't found at the expected path,
//! allowing CI and machines without RA2 to run `cargo test` without failures.

use std::path::{Path, PathBuf};

use crate::assets::mix_archive::MixArchive;
use crate::assets::mix_hash::{mix_hash, westwood_hash};

/// Helper: return the asset path if it exists, or skip the test.
/// Uses a macro so the `return` exits the calling test function.
macro_rules! skip_if_missing {
    ($path:expr) => {{
        let p: &Path = Path::new($path);
        if !p.exists() {
            eprintln!("SKIPPED: {} not found (RA2 assets not installed)", $path);
            return;
        }
        p
    }};
}

/// Helper: return the RA2 asset directory or skip the test.
/// Reads the path from RA2_DIR env var, or falls back to config.toml.
macro_rules! require_ra2_dir {
    () => {{
        let dir: PathBuf = match std::env::var("RA2_DIR") {
            Ok(val) => PathBuf::from(val),
            Err(_) => match crate::test_fixture::game_config() {
                Ok(cfg) => cfg.paths.ra2_dir,
                Err(_) => {
                    eprintln!("SKIPPED: RA2_DIR not set and config.toml not found");
                    return;
                }
            },
        };
        if !dir.exists() {
            eprintln!("SKIPPED: RA2 assets not found at {}", dir.display());
            return;
        }
        dir
    }};
}

/// Diagnostic: dump all ra2.mix entry IDs, probe their content, and brute-force filenames.
#[test]
fn test_diag_ra2_mix_hash_check() {
    let dir: PathBuf = require_ra2_dir!();
    let path: PathBuf = dir.join("ra2.mix");
    skip_if_missing!(path.to_str().unwrap());

    let archive: MixArchive = MixArchive::load(&path).expect("Should load ra2.mix");
    let entries = archive.entries();
    eprintln!("ra2.mix: {} entries", entries.len());

    // Probe each entry: peek at first 16 bytes to identify file type.
    for e in entries {
        let data: Option<&[u8]> = archive.get_by_id(e.id);
        let type_hint: String = match data {
            Some(d) if d.len() >= 4 => {
                let w0: u16 = u16::from_le_bytes([d[0], d[1]]);
                let w1: u16 = u16::from_le_bytes([d[2], d[3]]);
                let first4: [u8; 4] = [d[0], d[1], d[2], d[3]];
                if w0 == 0 && (w1 & 0x0002) != 0 {
                    "NEW-MIX(encrypted)".to_string()
                } else if w0 == 0 && w1 == 0 {
                    "NEW-MIX(plain) or zeros".to_string()
                } else if d.starts_with(b"[") {
                    "INI file".to_string()
                } else if d.starts_with(b"\x89PNG") {
                    "PNG image".to_string()
                } else if d.len() >= 768 && e.size == 768 {
                    "PAL (768 bytes)".to_string()
                } else if w0 > 0 && w0 < 5000 && e.size > 1000 {
                    // Old-format MIX: first u16 is file_count
                    let body_size: u32 = if d.len() >= 6 {
                        u32::from_le_bytes([d[2], d[3], d[4], d[5]])
                    } else {
                        0
                    };
                    format!("OLD-MIX?(count={},body={})", w0, body_size)
                } else {
                    format!(
                        "bytes={:02X}{:02X}{:02X}{:02X}",
                        first4[0], first4[1], first4[2], first4[3]
                    )
                }
            }
            Some(d) => format!("tiny({} bytes)", d.len()),
            None => "NOT EXTRACTABLE".to_string(),
        };
        eprintln!(
            "  id={:#010X}, size={:>10}, type={}",
            e.id as u32, e.size, type_hint
        );
    }

    // Comprehensive filename list (200+ known RA2/TS/YR filenames)
    let names: &[&str] = &[
        // Nested MIX archives
        "local.mix",
        "conquer.mix",
        "cache.mix",
        "cameo.mix",
        "cameos.mix",
        "snow.mix",
        "temperat.mix",
        "urban.mix",
        "desert.mix",
        "lunar.mix",
        "newurban.mix",
        "generic.mix",
        "neutral.mix",
        "isosnow.mix",
        "isotem.mix",
        "isourb.mix",
        "isodes.mix",
        "isolun.mix",
        "isoneub.mix",
        "isogen.mix",
        // YR nested
        "localmd.mix",
        "conqmd.mix",
        "cachemd.mix",
        "cameomd.mix",
        "snowmd.mix",
        "temperatmd.mix",
        "urbanmd.mix",
        "isosnowmd.mix",
        "isotemmd.mix",
        "isourbmd.mix",
        // Map packs
        "maps01.mix",
        "maps02.mix",
        "maps03.mix",
        "multi.mix",
        "mplayer.mix",
        "multimd.mix",
        "mapsmd01.mix",
        "mapsmd02.mix",
        "mapsmd03.mix",
        // Campaign / missions
        "missions.mix",
        "tutorial.mix",
        "sidec01.mix",
        "sidec02.mix",
        "sidec03.mix",
        // Score / audio
        "scores.mix",
        "audio.mix",
        // INI files at top level
        "rules.ini",
        "art.ini",
        "ai.ini",
        "sound.ini",
        "eva.ini",
        "battle.ini",
        "temperat.ini",
        "snow.ini",
        "urban.ini",
        "rulesmd.ini",
        "artmd.ini",
        "aimd.ini",
        "battlemd.ini",
        // String tables
        "ra2.csf",
        "ra2md.csf",
        // Audio index
        "audio.bag",
        "audio.idx",
        "audiomd.bag",
        "audiomd.idx",
        // Palettes
        "unittem.pal",
        "unit.pal",
        "temperat.pal",
        "isotem.pal",
        "unitsno.pal",
        "uniturb.pal",
        "snow.pal",
        "urban.pal",
        "isosnow.pal",
        "isourb.pal",
        "mousepal.pal",
        "grfxtd.pal",
        "anim.pal",
        "lib.pal",
        "grftxtd.pal",
        "theater.pal",
        // Common SHP/sprites
        "mouse.shp",
        "pips.shp",
        "power.shp",
        "clock.shp",
        // Expansion packs
        "expandmd01.mix",
        "expand01.mix",
        "expand02.mix",
        "expandmd02.mix",
        "expandmd03.mix",
        // Misc
        "movies01.mix",
        "movies02.mix",
        "movmd01.mix",
        "movmd02.mix",
        "theme.ini",
        "thememd.ini",
        // Tiberian Sun names (some RA2 installs share)
        "tibsun.mix",
        "conquer.mix",
        // ccmixar database names
        "multimix.mix",
        "ra2.mix",
        "langmd.mix",
        "language.mix",
    ];

    eprintln!("\nFilename matches:");
    let mut found: usize = 0;
    let mut matched_ids: Vec<i32> = Vec::new();
    for &name in names {
        let hash: i32 = mix_hash(name);
        if let Some(e) = entries.iter().find(|e| e.id == hash) {
            eprintln!(
                "  FOUND: {:25} -> id={:#010X}, size={}",
                name, hash as u32, e.size
            );
            found += 1;
            matched_ids.push(e.id);
        }
    }

    // Show unmatched entries
    eprintln!("\nUnmatched entries:");
    for e in entries {
        if !matched_ids.contains(&e.id) {
            eprintln!("  UNKNOWN: id={:#010X}, size={}", e.id as u32, e.size);
        }
    }
    eprintln!(
        "Hash lookup: {}/{} matched, {} unmatched",
        found,
        entries.len(),
        entries.len() - found
    );

    // Print the 151-byte INI file content
    let ini_id: i32 = 0x763C81DDu32 as i32;
    if let Some(data) = archive.get_by_id(ini_id) {
        let text: String = String::from_utf8_lossy(data).to_string();
        eprintln!("\n151-byte INI content:\n---\n{}\n---", text);
    }

    // Try filenames with path prefixes and alternate patterns
    let alt_names: &[&str] = &[
        "ra2\\local.mix",
        "ra2/local.mix",
        "ra2\\conquer.mix",
        "local",
        "conquer",
        "cache",
        "cameo",
        "LOCAL.MIX",
        "CONQUER.MIX",
        "CACHE.MIX",
        "RA2\\LOCAL.MIX",
        "mix\\local.mix",
        // Maybe numeric names?
        "0.mix",
        "1.mix",
        "2.mix",
        "3.mix",
        // Maybe the IDs are from Tiberian Sun hash?
    ];
    eprintln!("\nAlternate name attempts:");
    for &name in alt_names {
        let hash: i32 = mix_hash(name);
        if entries.iter().any(|e| e.id == hash) {
            eprintln!("  ALT MATCH: {} -> id={:#010X}", name, hash as u32);
        }
    }
}

/// Diagnostic: deep-dive into nested archive contents and hash matching.
#[test]
fn test_diag_asset_manager_total_entries() {
    let dir: PathBuf = require_ra2_dir!();

    let ra2_path: PathBuf = dir.join("ra2.mix");
    skip_if_missing!(ra2_path.to_str().unwrap());
    let ra2_mix: MixArchive = MixArchive::load(&ra2_path).expect("load ra2.mix");

    // Find the largest nested archive (likely local.mix or similar)
    let mut biggest_entry: Option<&crate::assets::mix_archive::MixEntry> = None;
    for entry in ra2_mix.entries() {
        if entry.size < 1000 {
            continue;
        }
        if biggest_entry.is_none_or(|b| entry.size > b.size) {
            biggest_entry = Some(entry);
        }
    }

    let big: &crate::assets::mix_archive::MixEntry = biggest_entry.expect("Should have entries");
    eprintln!(
        "Largest entry: id={:#010X}, size={:.1} MB",
        big.id as u32,
        big.size as f64 / (1024.0 * 1024.0)
    );

    let big_data: &[u8] = ra2_mix.get_by_id(big.id).expect("extract");
    let big_archive: MixArchive = MixArchive::from_bytes(big_data.to_vec()).expect("parse as MIX");

    eprintln!("Entries: {}", big_archive.entry_count());

    // Dump first 10 entries: ID, size, and first 8 bytes of content.
    let entries = big_archive.entries();
    for (i, e) in entries.iter().take(10).enumerate() {
        let content: Option<&[u8]> = big_archive.get_by_id(e.id);
        let preview: String = match content {
            Some(d) if d.len() >= 8 => {
                format!(
                    "{:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
                    d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]
                )
            }
            Some(d) => format!("{} bytes", d.len()),
            None => "EXTRACTION FAILED".to_string(),
        };
        eprintln!(
            "  [{}] id={:#010X}, size={:>8}, first8=[{}]",
            i, e.id as u32, e.size, preview
        );
    }

    // Compute expected CRC-32 hashes for key filenames.
    eprintln!("\nExpected CRC-32 hashes for key filenames:");
    let key_names: &[&str] = &[
        "clat01.tem",
        "s1.tem",
        "rules.ini",
        "art.ini",
        "mouse.shp",
        "unittem.pal",
        "isotem.pal",
        "temperat.pal",
    ];
    for &name in key_names {
        let hash: i32 = mix_hash(name);
        let found: bool = entries.iter().any(|e| e.id == hash);
        eprintln!(
            "  {:20} -> {:#010X} (i32={:>12}) | {}",
            name,
            hash as u32,
            hash,
            if found { "FOUND" } else { "not in archive" }
        );
    }

    // Try BOTH hash algorithms on key filenames against the biggest archive.
    eprintln!("\nWestwood hash vs CRC-32 for key filenames (vs biggest archive):");
    let test_names2: &[&str] = &[
        "clat01.tem",
        "s1.tem",
        "rules.ini",
        "art.ini",
        "mouse.shp",
        "unittem.pal",
        "isotem.pal",
        "local.mix",
        "conquer.mix",
        "cache.mix",
    ];
    for &name in test_names2 {
        let crc: i32 = mix_hash(name);
        let ww: i32 = westwood_hash(name);
        let crc_found: bool = entries.iter().any(|e| e.id == crc);
        let ww_found: bool = entries.iter().any(|e| e.id == ww);
        eprintln!(
            "  {:20} CRC32={:#010X} {} | WW={:#010X} {}",
            name,
            crc as u32,
            if crc_found { "FOUND!" } else { "-" },
            ww as u32,
            if ww_found { "FOUND!" } else { "-" },
        );
    }

    // Try Westwood hash against ALL ra2.mix entries too.
    let ra2_entries = ra2_mix.entries();
    eprintln!("\nWestwood hash vs ra2.mix top-level:");
    let top_names: &[&str] = &[
        "local.mix",
        "conquer.mix",
        "cache.mix",
        "cameo.mix",
        "snow.mix",
        "temperat.mix",
        "urban.mix",
        "isosnow.mix",
        "isotem.mix",
        "isourb.mix",
        "generic.mix",
        "multi.mix",
        "maps01.mix",
        "rules.ini",
        "art.ini",
        "key.ini",
        "scores.mix",
    ];
    for &name in top_names {
        let crc: i32 = mix_hash(name);
        let ww: i32 = westwood_hash(name);
        let crc_found: bool = ra2_entries.iter().any(|e| e.id == crc);
        let ww_found: bool = ra2_entries.iter().any(|e| e.id == ww);
        if crc_found || ww_found {
            eprintln!(
                "  {:20} CRC32={:#010X} {} | WW={:#010X} {}",
                name,
                crc as u32,
                if crc_found { "FOUND!" } else { "-" },
                ww as u32,
                if ww_found { "FOUND!" } else { "-" },
            );
        }
    }
}

/// Diagnostic: probe ra2md.mix for nested YR archives.
#[test]
fn test_diag_ra2md_mix() {
    let dir: PathBuf = require_ra2_dir!();
    let path: PathBuf = dir.join("ra2md.mix");
    skip_if_missing!(path.to_str().unwrap());

    let archive: MixArchive = MixArchive::load(&path).expect("Should load ra2md.mix");
    eprintln!("ra2md.mix: {} entries", archive.entry_count());

    let all_names: &[&str] = &[
        "localmd.mix",
        "conqmd.mix",
        "snowmd.mix",
        "temperatmd.mix",
        "local.mix",
        "conquer.mix",
        "snow.mix",
        "temperat.mix",
    ];
    for &name in all_names {
        if archive.get_by_name(name).is_some() {
            eprintln!("  {} FOUND", name);
        }
    }
}
