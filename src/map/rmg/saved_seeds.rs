//! Saved random-map seeds: the `.SED` files the setup dialog's Load / Save /
//! Delete buttons browse.
//!
//! Metadata enumeration and persistence belong here; the browser owns selection,
//! descriptions and transactions. New filenames come from the shared CRT stream.

use std::path::Path;

use super::options::RmgOptions;
use crate::util::native_file_name::{self, NativeFileName};
use std::io::{Read, Write};

/// The extension every saved seed carries. Matched case-insensitively — the
/// engine writes mixed case and players' files come from anywhere.
pub const SEED_EXTENSION: &str = "sed";

/// Names that live in the same directory with the same extension but are not
/// player-visible saves:
/// - the setup dialog's own working file, rewritten on every accept,
/// - the engine's last-played scratch copy,
/// - the network save, which is not a seed at all.
///
/// Listing any of these would offer the player a "save" that the engine
/// overwrites behind their back.
pub const RESERVED_SEED_NAMES: [&str; 3] = ["randmap.sed", "lastmap.sed", "savegame.net"];

/// Whether a file name is one of the reserved, non-browsable ones.
pub fn is_reserved_seed_name(file_name: &str) -> bool {
    let lowered = file_name.to_ascii_lowercase();
    RESERVED_SEED_NAMES.contains(&lowered.as_str())
}

/// Whether a file name is a browsable saved seed.
pub fn is_browsable_seed(file_name: &str) -> bool {
    Path::new(file_name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(SEED_EXTENSION))
        && !is_reserved_seed_name(file_name)
}

/// Metadata accepted by MapSeed's callback, including invalid descriptions.
///
/// Native 0x00597D60 returns true for an empty description but marks it invalid.
/// RebuildEntryList 0x005596A0 sorts these records before omitting invalid rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedSeed {
    pub file_name: NativeFileName,
    pub description: super::SeedDescription,
    /// Windows FILETIME ticks (100 ns since 1601), from the enumeration record.
    pub last_write_time: u64,
}

impl SavedSeed {
    pub fn is_valid(&self) -> bool {
        !self.description.is_empty()
    }
}

/// Enumerate accepted metadata in filesystem order. The browser adds its New
/// row before sorting the complete array; availability only tests valid data.
pub fn list_saved_seeds(dir: &Path) -> Vec<SavedSeed> {
    enumerate_seed_files(dir)
        .into_iter()
        .filter_map(|(name, attributes, time)| {
            if attributes & 0x116 != 0
                || RESERVED_SEED_NAMES
                    .iter()
                    .any(|reserved| name.is_ascii_name(reserved.as_bytes()))
            {
                return None;
            }
            // Metadata 0x00597E24 rejects INI load failure before sorting.
            let mut bytes = Vec::new();
            native_file_name::open(dir, &name, false)
                .ok()?
                .read_to_end(&mut bytes)
                .ok()?;
            if !metadata_ini_has_section(&bytes) {
                return None;
            }
            let ini = crate::rules::ini_parser::IniFile::from_bytes(&bytes).ok();
            let raw = ini
                .as_ref()
                .and_then(|ini| ini.section("RandomMap"))
                .and_then(|section| section.get("Description"));
            Some(SavedSeed {
                // Read using the full enumeration name; actions use the bounded
                // copy at 0x00597E96, even if the boundary splits an ANSI character.
                file_name: name.truncated(32),
                description: super::description::read_description(
                    raw,
                    &super::SeedDescription::default(),
                ),
                last_write_time: time,
            })
        })
        .collect()
}

/// Fresh native INI load rejects EOF before its first recognized section
/// (0x00525AFF); a final unterminated header is not processed. This gate covers
/// ordinary SED text. Exotic malformed/BOM line-reader behavior is not certified.
fn metadata_ini_has_section(bytes: &[u8]) -> bool {
    bytes.split_inclusive(|byte| *byte == b'\n').any(|line| {
        if line.last() != Some(&b'\n') {
            return false;
        }
        let start = line
            .iter()
            .position(|byte| *byte > 0x20)
            .unwrap_or(line.len());
        let line = &line[start..];
        line.first() == Some(&b'[') && line.contains(&b']')
    })
}

#[cfg(windows)]
fn enumerate_seed_files(dir: &Path) -> Vec<(NativeFileName, u32, u64)> {
    native_file_name::enumerate(dir, "*.SED")
}

#[cfg(not(windows))]
fn enumerate_seed_files(dir: &Path) -> Vec<(NativeFileName, u32, u64)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if !is_browsable_seed(&name) {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            Some((
                name.into(),
                0,
                metadata
                    .modified()
                    .map(system_time_to_file_time)
                    .unwrap_or_default(),
            ))
        })
        .collect()
}

pub fn read_browser_seed(
    dir: &Path,
    name: &NativeFileName,
    current: &RmgOptions,
    default: &str,
) -> std::io::Result<RmgOptions> {
    let mut bytes = Vec::new();
    native_file_name::open(dir, name, false)?.read_to_end(&mut bytes)?;
    Ok(options_from_bytes(&bytes, current, default))
}

pub fn write_browser_seed(
    dir: &Path,
    name: &NativeFileName,
    options: &RmgOptions,
) -> std::io::Result<()> {
    native_file_name::open(dir, name, true)?.write_all(&options.to_sed_bytes())
}

pub fn system_time_to_file_time(time: std::time::SystemTime) -> u64 {
    const UNIX_EPOCH_TICKS: u64 = 116_444_736_000_000_000;
    match time.duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => UNIX_EPOCH_TICKS
            .saturating_add(u64::try_from(duration.as_nanos() / 100).unwrap_or(u64::MAX)),
        Err(before) => UNIX_EPOCH_TICKS
            .saturating_sub(u64::try_from(before.duration().as_nanos() / 100).unwrap_or(u64::MAX)),
    }
}

/// New's synthetic timestamp comes from GetSystemTime, which has millisecond
/// fields (0x00559806), while file metadata retains full FILETIME precision.
pub fn new_slot_file_time(time: std::time::SystemTime) -> u64 {
    let ticks = system_time_to_file_time(time);
    ticks - ticks % 10_000
}

/// Native availability 0x00559C20 requires at least one non-invalid entry.
pub fn saved_seeds_available(dir: &Path) -> bool {
    list_saved_seeds(dir).iter().any(SavedSeed::is_valid)
}

/// New-file allocation at 0x005592B0. The app supplies the shared UI-thread CRT
/// stream and the original MIX-then-raw availability policy.
pub fn allocate_seed_file_name(
    random: &mut crate::util::legacy_crt_rng::LegacyCrtRng,
    mut available: impl FnMut(&str) -> bool,
) -> NativeFileName {
    loop {
        let name = format!("SAVE{:04X}.SED", random.draw15());
        if !available(&name) {
            return name.into();
        }
    }
}

/// Read a saved seed's options.
///
/// MapSeedClass load slot 0x00597A30 overlays the current record: missing keys
/// retain the player's working values. The setup caller at 0x005969B1 then
/// synchronizes controls through 0x00596E50, which normalizes the record.
/// See RANDOM_MAP_SAVED_SEED_SLOTS_GHIDRA_REPORT.md sections 3.5 and 3.6.
#[cfg(test)]
pub fn load_saved_seed(
    path: &Path,
    current: &RmgOptions,
    default_description: &str,
) -> std::io::Result<RmgOptions> {
    let bytes = std::fs::read(path)?;
    Ok(options_from_bytes(&bytes, current, default_description))
}

fn options_from_bytes(bytes: &[u8], current: &RmgOptions, default_description: &str) -> RmgOptions {
    let mut options = current.clone();
    // Unlike the integer keys, Description uses the localized default passed
    // at 0x00597AFD to INIClass__ReadCommaHexUTF16, not the current description.
    options.description = default_description.into();
    if let Ok(ini) = crate::rules::ini_parser::IniFile::from_bytes(&bytes) {
        options.apply_sed(&ini);
    }
    options.normalize();
    options
}

/// Write a saved seed.
#[cfg(test)]
pub fn save_saved_seed(path: &Path, options: &RmgOptions) -> std::io::Result<()> {
    std::fs::write(path, options.to_sed_bytes())
}

/// Delete a saved seed.
#[cfg(test)]
pub fn delete_saved_seed(path: &Path) -> std::io::Result<()> {
    std::fs::remove_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_requires_a_recognized_complete_section_line() {
        for bytes in [b"".as_slice(), b"plain text\n", b"[RandomMap]"] {
            assert!(!metadata_ini_has_section(bytes));
        }
        for bytes in [b"[RandomMap]\n".as_slice(), b"; comment\r\n[RandomMap]\r\n"] {
            assert!(metadata_ini_has_section(bytes));
        }
    }

    #[test]
    fn disk_load_uses_native_description_results_without_changing_numeric_defaults() {
        let path = std::env::temp_dir().join(format!(
            "vera20k-seed-description-{}-{}.sed",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let current = RmgOptions {
            seed: 23,
            width: 2,
            description: "Prior map".into(),
            ..RmgOptions::default()
        };
        for (encoded, expected) in [
            (None, "Zufallskarte"),
            (Some(""), "Zufallskarte"),
            (Some("   "), "Zufallskarte"),
            (Some(",,,"), ""),
            (Some("z"), "\u{b573}"),
            (Some("41,z,42,"), "AAB"),
            (Some("41,0,42,"), "A"),
            (Some("0x41,+42,"), "AB"),
        ] {
            let mut ini = "[RandomMap]\nSeed=42\n".to_owned();
            if let Some(encoded) = encoded {
                ini.push_str(&format!("Description={encoded}\n"));
            }
            std::fs::write(&path, &ini).unwrap();
            let loaded = load_saved_seed(&path, &current, "Zufallskarte").unwrap();
            assert_eq!(loaded.description, expected, "{encoded:?}");
            assert_eq!(loaded.seed, 42);
            assert_eq!(loaded.width, current.width);
            // The other production consumer, loading/init.rs, parses a seed
            // and applies it to constructor defaults directly. Its current
            // empty Description fallback is preserved by this increment.
            let mut direct = RmgOptions::default();
            direct.apply_sed(&crate::rules::ini_parser::IniFile::from_str(&ini));
            direct.normalize();
            let direct_expected = if expected == "Zufallskarte" {
                ""
            } else {
                expected
            };
            assert_eq!(direct.description, direct_expected, "direct {encoded:?}");
            assert_eq!(direct.seed, 42);
        }
        assert_eq!(current.description, "Prior map");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn partial_seed_preserves_current_integers_and_uses_localized_description_default() {
        let path = std::env::temp_dir().join(format!(
            "vera20k-partial-seed-{}-{}.sed",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &path,
            b"[RandomMap]\nSeed=4660\nWaterAmount=73\nNumPlayers=99\n",
        )
        .unwrap();
        let current = RmgOptions {
            theater: 1,
            map_type: 3,
            resources: 2,
            ruggedness: 67,
            time: 3,
            water_amount: 31,
            num_players: 6,
            tiberium: 47,
            tiberium_layout: 19,
            vegetation: 29,
            urban_presence: 13,
            width: 2,
            height: 1,
            accessibility: 89,
            region_size: 83,
            seed: 42,
            description: "Prior map".into(),
        };
        let loaded = load_saved_seed(&path, &current, "Localized random map").unwrap();
        let expected = RmgOptions {
            seed: 4660,
            water_amount: 73,
            num_players: 8,
            description: "Localized random map".into(),
            ..current.clone()
        };
        assert_eq!(loaded, expected);
        // Description has its own comma-hex reader/default contract.
        std::fs::write(&path, b"[RandomMap]\nDescription=53,61,76,65,64,\n").unwrap();
        assert_eq!(
            load_saved_seed(&path, &current, "fallback")
                .unwrap()
                .description,
            "Saved"
        );
        std::fs::remove_file(&path).unwrap();
        assert!(load_saved_seed(&path, &current, "fallback").is_err());
        assert_eq!(
            current.seed, 42,
            "loading never mutates the supplied record"
        );
    }

    #[test]
    fn reserved_names_are_matched_regardless_of_case() {
        for name in ["RandMap.Sed", "RANDMAP.SED", "randmap.sed", "LastMap.sed"] {
            assert!(is_reserved_seed_name(name), "{name} is reserved");
            assert!(!is_browsable_seed(name), "{name} is not browsable");
        }
    }

    #[test]
    fn only_sed_files_are_browsable() {
        assert!(is_browsable_seed("mymap.sed"));
        assert!(is_browsable_seed("MyMap.SED"));
        assert!(!is_browsable_seed("mymap.map"));
        assert!(!is_browsable_seed("mymap"));
        assert!(!is_browsable_seed("savegame.net"));
    }

    #[test]
    fn listing_a_missing_directory_is_empty_rather_than_an_error() {
        let seeds = list_saved_seeds(Path::new("C:/definitely/not/here"));
        assert!(seeds.is_empty());
        assert!(!saved_seeds_available(Path::new("C:/definitely/not/here")));
    }

    #[test]
    fn a_saved_seed_round_trips_through_disk() {
        let dir = std::env::temp_dir().join("vera20k_saved_seed_roundtrip");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");

        let options = RmgOptions {
            seed: 4242,
            num_players: 6,
            map_type: 3,
            description: "Desert Duel".into(),
            ..Default::default()
        };
        let path = dir.join("SAVE1234.SED");
        save_saved_seed(&path, &options).expect("save");

        let listed = list_saved_seeds(&dir);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].description, "Desert Duel");

        let loaded = load_saved_seed(&path, &RmgOptions::default(), "Random Map").expect("load");
        assert_eq!(loaded.seed, 4242);
        assert_eq!(loaded.num_players, 6);
        assert_eq!(loaded.map_type, 3);
        assert_eq!(loaded.description, "Desert Duel");

        delete_saved_seed(&path).expect("delete");
        assert!(
            list_saved_seeds(&dir).is_empty(),
            "delete removes the entry"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_working_file_never_appears_in_the_list() {
        let dir = std::env::temp_dir().join("vera20k_saved_seed_reserved");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(dir.join("RandMap.Sed"), b"[RandomMap]\n").expect("write");
        std::fs::write(dir.join("lastmap.sed"), b"[RandomMap]\n").expect("write");
        std::fs::write(
            dir.join("Keeper.sed"),
            b"[RandomMap]\nDescription=4b,65,65,70,65,72,\n",
        )
        .expect("write");

        let listed = list_saved_seeds(&dir);
        assert_eq!(listed.len(), 1, "only the real save is listed: {listed:?}");
        assert_eq!(listed[0].description, "Keeper");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
