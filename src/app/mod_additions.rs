//! A mod's map additions: INI files that the game adds to the map of every
//! Skirmish and online match, such as an AI another author made.
//!
//! VERA-only. gamemd reads one map file; launchers merged such option files
//! into the map they handed the game. Here a player puts the file, as its
//! author ships it, in a folder of its own in `Mods` and picks that folder as
//! the mod (docs/modding.md). The folder counts only while it holds nothing
//! else the game reads: besides INI files the game has no use for, only notes
//! and pictures ([`NOTES`]), subfolders, VERA's own files
//! ([`mod_files::uncompared`]) and what systems leave there
//! ([`mods::left_by_a_system`]). A mod with any other file, such as its own
//! `rulesmd.ini` or an archive, adds nothing to maps.
//!
//! [`reach`] says which matches play them. `ProcessAssets::map_additions`
//! reads them once a start, so the matches play what the network rules
//! agreement compares, and the loader merges them into the map
//! (`MapLoadInitial::with_mod_additions`).
//!
//! [`mod_files::uncompared`]: crate::app::mod_files::uncompared
//! [`mods::left_by_a_system`]: crate::app::mods::left_by_a_system

use std::path::Path;

use crate::rules::ini_parser::IniFile;
use crate::util::file_system;

/// The most of one addition that is read: an AI's file holds a few hundred
/// KB.
const FILE_LIMIT: u64 = 8 * 1024 * 1024;

/// INI files that the game reads, or that a folder holds only as a mod of its
/// own, by name without case. Every name ending in `md.ini` counts too
/// ([`is_addition`]): Yuri's Revenge's own files, such as `rulesmd.ini`, its
/// mode and theater INIs, and a campaign mission's, such as `all04dmd.ini`
/// (`select_campaign_named_ini_for_full_init`). Here are the rest: the files
/// beside them, the Mental Omega names of the Rules, Art, AI, sound and
/// theater files (`util::ini_roots`), the files a launcher writes for a
/// match, VERA's settings, and Red Alert 2's own.
const GAME_INIS: [&str; 28] = [
    "langrule.ini",
    "tmcj4f.ini",
    "spawn.ini",
    "spawnmap.ini",
    "vera.ini",
    "verarules.ini",
    "rulesmo.ini",
    "artmo.ini",
    "aimo.ini",
    "soundmo.ini",
    "evamo.ini",
    "temperatmo.ini",
    "snowmo.ini",
    "urbanmo.ini",
    "desertmo.ini",
    "lunarmo.ini",
    "urbannmo.ini",
    "rules.ini",
    "art.ini",
    "ai.ini",
    "sound.ini",
    "eva.ini",
    "theme.ini",
    "battle.ini",
    "rmg.ini",
    "temperat.ini",
    "snow.ini",
    "urban.ini",
];

/// Extensions of notes and pictures, which the game never reads, so a folder
/// may hold them beside its additions, as it may a file without an extension,
/// such as a `LICENSE`.
const NOTES: [&str; 10] = [
    "txt", "md", "pdf", "png", "jpg", "jpeg", "gif", "url", "htm", "html",
];

/// A match plays a mod's map additions unless it is a campaign mission, a
/// cooperative one too, or on a random map, which reads its rules while it is
/// made.
pub(crate) fn reach(startup: &crate::app::match_bootstrap::LoadingStartup) -> bool {
    startup.session_mode().is_nonzero()
        && !crate::map::rmg::is_seed_selection(startup.physical_map_file())
        && !startup.launch_session().is_some_and(|launch| {
            launch.mode.class == crate::skirmish_modes::MpModeClass::Cooperative
        })
}

/// What a mod's folder adds to each map, in the order the game lists the
/// folder: a later file's keys win over an earlier one's.
#[derive(Debug)]
pub(crate) struct MapAdditions {
    names: Vec<String>,
    files: Vec<IniFile>,
}

impl MapAdditions {
    /// The additions of the mod in `dir`; `None` when the folder holds none,
    /// holds another file the game reads (see the module), or cannot be
    /// listed. `roots` are the INI files this game reads under names
    /// `config.toml` can pick: the Rules, Art, AI, sound and theater files. An
    /// addition that cannot be read is the error, which a player is shown.
    pub(crate) fn read(dir: &Path, roots: &[String]) -> Result<Option<Self>, String> {
        let listing = match crate::util::case_insensitive_path::native_listing(dir) {
            Ok(listing) => listing,
            Err(error) => {
                log::warn!("Could not list the mod in {}: {error}", dir.display());
                return Ok(None);
            }
        };
        let mut found = Vec::new();
        let mut game_file = None;
        for path in listing {
            let Some(name) = path.file_name().map(|name| name.to_string_lossy()) else {
                continue;
            };
            // As the pre-match check compares a mod's files: only what can
            // be read as a file counts.
            if !file_system::metadata(&path).is_ok_and(|entry| entry.is_file())
                || crate::app::mods::left_by_a_system(&name)
                || crate::app::mod_files::uncompared(&name)
                || is_note(&name)
            {
                continue;
            }
            if is_addition(&name, roots) {
                found.push((name.into_owned(), path));
            } else {
                game_file.get_or_insert(name.into_owned());
            }
        }
        if let Some(game_file) = game_file {
            if !found.is_empty() {
                log::info!("The mod holds {game_file}, so it adds no INI files to maps");
            }
            return Ok(None);
        }
        if found.is_empty() {
            return Ok(None);
        }
        let mut additions = Self {
            names: Vec::with_capacity(found.len()),
            files: Vec::with_capacity(found.len()),
        };
        for (name, path) in found {
            let file = read_file(&path).map_err(|error| {
                format!(
                    "The mod's {name} could not be read: {error}. Fix the file, or pick \
                     another mod under Options, Mods."
                )
            })?;
            additions.names.push(name);
            additions.files.push(file);
        }
        Ok(Some(additions))
    }

    pub(crate) fn names(&self) -> &[String] {
        &self.names
    }

    pub(crate) fn files(&self) -> &[IniFile] {
        &self.files
    }

    /// What the additions hold, in their order, as one digest for the network
    /// rules agreement (`ProcessAssets::multiplayer_rules_digest`).
    pub(crate) fn digest(&self) -> String {
        let mut bytes = b"VERA20k map additions v1\0".to_vec();
        for file in &self.files {
            bytes.extend_from_slice(file.multiplayer_digest().as_bytes());
        }
        crate::util::sha256::sha256_hex(&bytes)
    }
}

fn is_note(name: &str) -> bool {
    Path::new(name).extension().is_none_or(|extension| {
        NOTES
            .iter()
            .any(|note| extension.eq_ignore_ascii_case(note))
    })
}

/// An INI file that the game has no use for ([`GAME_INIS`]).
fn is_addition(name: &str, roots: &[String]) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".ini")
        && !lower.ends_with("md.ini")
        && !GAME_INIS.contains(&lower.as_str())
        && !roots.iter().any(|root| root.eq_ignore_ascii_case(name))
}

fn read_file(path: &Path) -> Result<IniFile, String> {
    let size = file_system::metadata(path)
        .map_err(|error| error.to_string())?
        .len();
    if size > FILE_LIMIT {
        return Err(format!("it is larger than {} MB", FILE_LIMIT >> 20));
    }
    let mut bytes = file_system::read(path).map_err(|error| error.to_string())?;
    // The INI reader drops a last line that has no line end, as gamemd's
    // does; a launcher wrote the merged map with one.
    if bytes.last().is_some_and(|last| *last != b'\n') {
        bytes.push(b'\n');
    }
    IniFile::from_bytes(&bytes).map_err(|error| error.to_string())
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use crate::map::source::test_support::TestDirectory;

    #[test]
    fn a_folder_of_unread_ini_files_adds_them_and_a_game_file_makes_it_a_mod() {
        let dir = TestDirectory::new("map-additions");
        // As its author ships it: the last line has no line end.
        dir.write("AI teams.ini", b"[Basic]\nIgnoreGlobalAITriggers=yes");
        dir.write("AI bases.INI", b"[General]\nAIMinorSuperReadyPercent=1\n");
        for other in ["README.md", "LICENSE", "mod.ini", "veraui.ini", ".DS_Store"] {
            dir.write(other, b"[Mod]\nName=Hard\n");
        }
        std::fs::create_dir(dir.path().join("Screenshots")).unwrap();
        let roots = ["rulesx.ini".to_owned()];
        let additions = MapAdditions::read(dir.path(), &roots).unwrap().unwrap();
        assert_eq!(additions.names(), ["AI bases.INI", "AI teams.ini"]);
        let basic = additions.files()[1].section("Basic").unwrap();
        assert_eq!(basic.get_for_test("IgnoreGlobalAITriggers"), Some("yes"));
        // An archive, a campaign mission's INI, and a Rules file under the
        // name config.toml picked.
        for game_file in ["expandmd99.mix", "ALL04DMD.INI", "RulesX.ini"] {
            dir.write(game_file, b"[General]\n");
            let read = MapAdditions::read(dir.path(), &roots).unwrap();
            assert!(read.is_none(), "{game_file}");
            std::fs::remove_file(dir.path().join(game_file)).unwrap();
        }
    }
}
