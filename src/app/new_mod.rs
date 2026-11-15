//! The Mods page's New Mod: a mod folder that starts as the original game's
//! main INI files, for the player to change. VERA-only; gamemd has no Mods
//! folder. Each copy is the file the game folder gives with no mod in use,
//! from an archive or loose, so an unchanged New Mod plays as the original
//! game, and deleting its folder takes it away again (docs/modding.md).
//!
//! The Mods folder and its list are `app::mods`'; this only adds a folder.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The files New Mod copies, by the names the game reads them under: the
/// rules, the art, the AI, the sounds, EVA's voice and the music.
pub(crate) const COPIED: [&str; 6] = [
    "rulesmd.ini",
    "artmd.ini",
    "aimd.ini",
    "soundmd.ini",
    "evamd.ini",
    "thememd.ini",
];

/// The files New Mod writes for the player to fill in, every key commented
/// out so that each changes nothing until the player takes a `;` away.
const STARTERS: [(&str, &str); 2] = [
    // Every line `app::mods::ModInfo` reads, so the folder's name stays the
    // mod's name until the player gives it one.
    (
        "mod.ini",
        "; What the Mods menu shows for this mod. Every line is optional.\r\n\
         ; Take away the ; at the start of a line to use it.\r\n\
         [Mod]\r\n\
         ;Name=My Mod\r\n\
         ;Version=1.0\r\n\
         ;Author=Your name\r\n\
         ;About=What the mod changes, in a line or two.\r\n\
         ;Terms=What others may do with your work.\r\n",
    ),
    // `ui::match_layout`: with no `[Layouts]` every side keeps the original
    // interface. The example is docs/interface-layout.md's own.
    (
        "veraui.ini",
        "; The match screen's layout for this mod. While every line here starts\r\n\
         ; with ; the game shows the original layout.\r\n\
         ;\r\n\
         ; The guide explains every part and key, and has a full sample:\r\n\
         ; https://github.com/YuriPlanet/vera20k/blob/main/docs/interface-layout.md\r\n\
         ;\r\n\
         ; To try it, take away the ; at the start of the lines below. Every side\r\n\
         ; then gets the sidebar on the left and the command bar on top.\r\n\
         ;\r\n\
         ;[Layouts]\r\n\
         ;Default=Mine\r\n\
         ;\r\n\
         ;[Mine.Side]\r\n\
         ;Hold=Left\r\n\
         ;\r\n\
         ;[Mine.Bar]\r\n\
         ;Hold=Top\r\n",
    ),
];

/// The name a translation that is no plain folder name gives way to.
const FALLBACK_NAME: &str = "New Mod";

/// How many folders of one name New Mod numbers before it gives up.
const MOST_OF_A_NAME: u32 = 99;

/// The folder New Mod made.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Made {
    /// Its name in `Mods`.
    pub(crate) folder: String,
    pub(crate) dir: PathBuf,
    /// The files of [`COPIED`] that the game gave.
    pub(crate) copied: Vec<&'static str>,
}

/// Make a folder in `mods_dir` named `name`, or `name 2`, `name 3` and on
/// while one of that name is there in any case, and copy into it each file of
/// [`COPIED`] that `original` gives, with the [`STARTERS`] to fill in. Never writes
/// into a folder or over a file that was there; on a failure it takes away
/// what it wrote.
pub(crate) fn make(
    mods_dir: &Path,
    name: &str,
    original: impl Fn(&str) -> Option<Vec<u8>>,
) -> io::Result<Made> {
    let name = plain_name(name);
    let (folder, dir) = (1..=MOST_OF_A_NAME)
        .map(|n| match n {
            1 => name.to_owned(),
            n => format!("{name} {n}"),
        })
        .find_map(|folder| {
            // A folder that differs only in case would be the same mod.
            if crate::util::case_insensitive_path::find(mods_dir, &folder).is_some() {
                return None;
            }
            let dir = mods_dir.join(&folder);
            match std::fs::create_dir(&dir) {
                Ok(()) => Some(Ok((folder, dir))),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => None,
                Err(error) => Some(Err(error)),
            }
        })
        .unwrap_or_else(|| Err(io::Error::other("every New Mod name is taken")))?;
    let mut copied = Vec::new();
    let filled = (|| {
        for file in COPIED {
            match original(file) {
                Some(bytes) => {
                    write_new(&dir.join(file), &bytes)?;
                    copied.push(file);
                }
                None => log::warn!("New Mod: the game has no {file}"),
            }
        }
        if copied.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "the game gave none of the files New Mod copies",
            ));
        }
        STARTERS
            .iter()
            .try_for_each(|(file, text)| write_new(&dir.join(file), text.as_bytes()))
    })();
    if let Err(error) = filled {
        // Nothing else is in a folder made a moment ago.
        for file in COPIED.iter().chain(STARTERS.iter().map(|(file, _)| file)) {
            let _ = std::fs::remove_file(dir.join(file));
        }
        let _ = std::fs::remove_dir(&dir);
        return Err(error);
    }
    Ok(Made {
        folder,
        dir,
        copied,
    })
}

/// `name` when it can be one folder's name on every system, else
/// [`FALLBACK_NAME`].
fn plain_name(name: &str) -> &str {
    let name = name.trim();
    let plain = !name.is_empty()
        && !name.starts_with('.')
        && !name.ends_with('.')
        && !name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']);
    if plain { name } else { FALLBACK_NAME }
}

fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(bytes)
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    /// A settings folder of the test's own, with an empty `Mods` in it.
    fn settings(label: &str) -> (PathBuf, crate::util::config::PathsConfig) {
        let root =
            std::env::temp_dir().join(format!("vera-new-mod-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Mods")).unwrap();
        let config = format!(
            "[paths]\nra2_dir = '{}'\nsettings_dir = '{}'\n",
            root.join("game").display(),
            root.display()
        );
        std::fs::write(root.join("config.toml"), config).unwrap();
        let paths = crate::util::config::GameConfig::load_from(&root, &root.join("vera20k"))
            .unwrap()
            .paths;
        (root, paths)
    }

    /// A game that gives the rules and the art, and nothing else New Mod asks
    /// for.
    fn original(file: &str) -> Option<Vec<u8>> {
        match file {
            "rulesmd.ini" => Some(b"[General]\r\n".to_vec()),
            "artmd.ini" => Some(b"[Movies]\r\n".to_vec()),
            _ => None,
        }
    }

    /// The new folder holds the game's copies and a mod.ini that names
    /// nothing, so the Mods list shows the folder's name; a folder of that
    /// name in another case is left as it was, and the next gets a number.
    #[test]
    fn new_mod_copies_the_games_files_into_a_folder_of_its_own() {
        let (root, paths) = settings("copies");
        let mods = paths.mods_dir();
        std::fs::create_dir(mods.join("new mod")).unwrap();
        std::fs::write(mods.join("new mod").join("rulesmd.ini"), b"mine").unwrap();

        let made = make(&mods, "New Mod", original).unwrap();

        assert_eq!(made.folder, "New Mod 2");
        assert_eq!(made.copied, ["rulesmd.ini", "artmd.ini"]);
        assert_eq!(
            std::fs::read(made.dir.join("rulesmd.ini")).unwrap(),
            b"[General]\r\n"
        );
        assert!(!made.dir.join("aimd.ini").exists());
        assert_eq!(
            std::fs::read(mods.join("new mod").join("rulesmd.ini")).unwrap(),
            b"mine"
        );
        let listed = crate::app::mods::installed(&paths);
        let row = listed
            .iter()
            .find(|row| row.folder.as_deref() == Some("New Mod 2"))
            .unwrap();
        assert_eq!(
            (row.title(), row.author.as_deref()),
            ("New Mod 2".into(), None)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The new veraui.ini keeps the original layout for every side, and its
    /// example, once its lines are uncommented, is a layout with no problem.
    #[test]
    fn new_mods_layout_file_changes_nothing_until_its_example_is_uncommented() {
        use crate::ui::match_layout::MatchLayoutFile;
        let (_, text) = STARTERS[1];
        let as_written = MatchLayoutFile::from_bytes(text.as_bytes());
        assert!(as_written.for_side(Some("GDI")).is_none());
        assert!(as_written.problems().is_empty());
        let uncommented: String = text
            .lines()
            .map(|line| match line.strip_prefix(';') {
                Some(rest) if rest.starts_with('[') || rest.contains('=') => rest,
                _ => line,
            })
            .flat_map(|line| [line, "\n"])
            .collect();
        let example = MatchLayoutFile::from_bytes(uncommented.as_bytes());
        assert!(example.for_side(Some("GDI")).is_some());
        assert!(example.problems().is_empty(), "{:?}", example.problems());
    }

    /// A game that gives none of the files leaves no folder behind, and a
    /// translated name that is no plain folder name gives way to New Mod.
    #[test]
    fn new_mod_leaves_nothing_when_the_game_gives_nothing() {
        let (root, paths) = settings("nothing");
        let mods = paths.mods_dir();
        let error = make(&mods, "Mods/New", |_| None).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(std::fs::read_dir(&mods).unwrap().next().is_none());
        assert_eq!(make(&mods, "a/b", original).unwrap().folder, FALLBACK_NAME);
        let _ = std::fs::remove_dir_all(&root);
    }
}
