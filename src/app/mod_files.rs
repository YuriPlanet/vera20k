//! A mod's files as a list: each file's path, its size and the SHA-256 of
//! what it holds.
//!
//! VERA-only; gamemd has no mods folder. Two things read a mod's files this
//! way. The pre-match check compares one digest of them ([`FileList::digest`]),
//! so players start a match only with the same files. And a mod can carry
//! the list as `mod-files.txt`, written by its maker, so a player's game can
//! say whether their copy is the mod as it was made: the Mods page's Check
//! Files ([`start`], `app::shell_mods`; docs/modding.md).
//!
//! The list in the folder is a claim, never proof: the pre-match check always
//! reads the files themselves.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};

use crate::util::file_system;

/// The list's file in a mod's folder.
pub(crate) const LIST_NAME: &str = "mod-files.txt";
/// The list's first line. A later form of the list gets another number, and
/// this build then says it cannot read the list.
const HEADER: &str = "VERA20K mod files 1";
/// The most of `mod-files.txt` that is read: [`FILES_LIMIT`] long lines.
const LIST_LIMIT: u64 = 16 * 1024 * 1024;

/// How deep below a mod's folder its files are compared: deeper than the game
/// reads, and an end to a folder that links back into itself.
const FILES_DEPTH: usize = 6;
/// Files in a mod's folder itself that players need not share, by
/// [`name_key`]: `mod.ini`, the mod's account of itself; `veraui.ini`, which
/// lays out the sidebar and only changes what a player sees
/// (docs/interface-layout.md); and the list, which only says what the others
/// are.
const UNCOMPARED: [&str; 3] = [
    "MOD.INI",
    crate::ui::match_layout::FILE_NAME,
    "MOD-FILES.TXT",
];
/// The folder in a mod's folder itself that holds an HD art pack, by
/// [`name_key`]: neither compared nor listed (`collect_files`).
const HD_ART_FOLDER: &str = "HD_ART";
/// `name`, a file in a mod's folder itself, is one that players need not
/// share ([`UNCOMPARED`]).
pub(crate) fn uncompared(name: &str) -> bool {
    UNCOMPARED.contains(&name_key(name).as_str())
}

/// Kinds of file, by the ending of their [`name_key`], that players of one
/// mod need not share ([`FileList::digest`]): what they hold only shows,
/// plays or tells one player something. They are text tables, fonts, movies,
/// the pictures only screens draw and documents. The simulation side (`sim`,
/// `rules`, `map`, `net`) reads no file of these kinds. A Rules, Art, AI or
/// mode INI or a map that includes one is compared as it was read, the
/// included file with it (`retail_sources::select_ini`). Where one of them
/// switches what else is read, another digest tells: in a folder of INI
/// additions it makes the folder a mod that adds none (`mod_additions`,
/// whose own notes are a shorter list), and beside a mod's parts it stops
/// their assembly, which changes every path (`util::mod_package`). A mod's
/// list still names them ([`FileList::scan`]).
///
/// RESIDUAL: a sound, EVA or theme INI that includes a file of these kinds
/// is no longer compared with it. Trigger: an Ares game whose such INI
/// names one in `[#include]`, and players whose copies of it differ. Effect:
/// as for the original game without a mod, whose sound, EVA and theme INIs
/// no room compares: a map trigger that takes such a list's number as a
/// value is refused at the start by the world it certifies. Rare.
///
/// Sounds, music and palettes are not among them. The sound, EVA and theme
/// lists number what a map's triggers name, a theme counts only while its
/// WAV is there (`rules::theme_ini`), and whether a palette loads decides a
/// building's death animation (`rules::effect_asset_catalog`). An archive
/// can hold any file, so every archive counts whatever it is named for.
const SHOWN_KINDS: [&str; 19] = [
    "CSF", "FNT", "BIK", "VQA", "PCX", "PNG", "JPG", "JPEG", "BMP", "GIF", "ICO", "TXT", "MD",
    "PDF", "RTF", "HTM", "HTML", "URL", "LOG",
];
/// The mouse pointer's shapes, which only the screen draws: the one file
/// among [`SHOWN_KINDS`] known by its whole name.
const SHOWN_NAMES: [&str; 1] = ["MOUSE.SHA"];
/// Whether the file under `key` only shows, plays or tells something
/// ([`SHOWN_KINDS`]). A name outside ASCII has no ending to tell by
/// ([`name_key`]) and counts as any other file.
fn only_shown(key: &str) -> bool {
    let name = key.rsplit('/').next().unwrap_or(key);
    SHOWN_NAMES.contains(&name)
        || name
            .rsplit_once('.')
            .is_some_and(|(_, kind)| SHOWN_KINDS.contains(&kind))
}

/// The most files of one mod that are compared. A mod has a few thousand;
/// folders that link into one another can list far more.
const FILES_LIMIT: usize = 50_000;

/// One file of a mod.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    /// Its path below the mod's folder as the file system spells it, `/`
    /// between the parts.
    path: String,
    /// The same path as it counts in a comparison: each part's [`name_key`].
    key: String,
    /// Its bytes: a download knows from the list how much is to come.
    size: u64,
    digest: [u8; 32],
}

/// A mod's files but the [`UNCOMPARED`], in the order of their keys.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct FileList {
    entries: Vec<Entry>,
}

/// Where a mod's folder and its list part.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Differences {
    /// Paths in the folder whose file holds something else than listed.
    pub(crate) changed: Vec<String>,
    /// Listed paths the folder lacks.
    pub(crate) missing: Vec<String>,
    /// Paths in the folder that the list does not name.
    pub(crate) extra: Vec<String>,
}

impl Differences {
    fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.missing.is_empty() && self.extra.is_empty()
    }
}

/// The files in a mod's folder for the pre-match check
/// (`ProcessAssets::multiplayer_rules_digest`), which compares their
/// [`FileList::digest`], and what the mod's list says of them, which the
/// rules parts report (`app::rules_parts`).
pub(crate) fn read_for_match(dir: &Path) -> std::io::Result<Checked> {
    check(dir, &AtomicBool::new(false))
}

impl FileList {
    /// Read every file in `dir`. Ends early, as an error, once `stop` is set.
    fn scan(dir: &Path, stop: &AtomicBool) -> std::io::Result<Self> {
        let mut files = Vec::new();
        collect_files(dir, ("", ""), FILES_DEPTH, &mut files)?;
        let mut entries = Vec::with_capacity(files.len());
        for (path, key, at) in files {
            if UNCOMPARED.contains(&key.as_str()) {
                continue;
            }
            if stop.load(Ordering::Relaxed) {
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            let (size, digest) = file_digest(&at)?;
            entries.push(Entry {
                path,
                key,
                size,
                digest,
            });
        }
        Ok(Self::sorted(entries))
    }

    fn sorted(mut entries: Vec<Entry>) -> Self {
        entries.sort_by(|a, b| (&a.key, &a.digest).cmp(&(&b.key, &b.digest)));
        Self { entries }
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// One digest of the files, for the pre-match check: players start a
    /// match only with the same files. Each file counts by its path below the
    /// folder ([`name_key`]) and by what it holds. The folder's own name and
    /// the order the file system lists in do not count, nor do the files in
    /// the folder itself that no match reads ([`UNCOMPARED`]), nor a file
    /// that only shows, plays or tells one player something
    /// ([`SHOWN_KINDS`]): players with another translation or other screens
    /// play together. Only what can be read as a file counts: a link to
    /// nothing, a pipe or a device is none of the mod's files.
    pub(crate) fn digest(&self) -> [u8; 32] {
        let mut all = crate::util::sha256::Sha256::new();
        all.update(b"VERA20k mod files v1\0");
        for entry in self.entries.iter().filter(|entry| !only_shown(&entry.key)) {
            all.update(&(entry.key.len() as u64).to_le_bytes());
            all.update(entry.key.as_bytes());
            all.update(&entry.digest);
        }
        all.finalize()
    }

    /// The start of [`FileList::digest`] in a form two players can read to
    /// each other, such as `4F2A-91C3`: the same code is the same compared
    /// files.
    pub(crate) fn code(&self) -> String {
        let [a, b, c, d, ..] = self.digest();
        format!("{a:02X}{b:02X}-{c:02X}{d:02X}")
    }

    /// The list as `mod-files.txt` holds it: [`HEADER`], then a line for each
    /// file with its SHA-256 in hex, its size in bytes and its path. `None`
    /// when a path cannot stand in a line ([`listable`]).
    fn text(&self) -> Option<String> {
        use std::fmt::Write;
        let mut text = format!("{HEADER}\n");
        for entry in &self.entries {
            if !listable(&entry.path) {
                return None;
            }
            for byte in entry.digest {
                let _ = write!(text, "{byte:02x}");
            }
            let _ = writeln!(text, " {} {}", entry.size, entry.path);
        }
        Some(text)
    }

    /// Read the text of a `mod-files.txt`, or say why it is not one.
    fn parse(text: &str) -> Result<Self, String> {
        let mut lines = text
            .trim_start_matches('\u{feff}')
            .lines()
            .map(|line| line.trim_end_matches('\r'))
            .enumerate()
            .filter(|(_, line)| !line.is_empty());
        if lines.next().map(|(_, line)| line.trim_end()) != Some(HEADER) {
            return Err(format!("the first line is not {HEADER:?}"));
        }
        let mut entries = Vec::new();
        for (index, line) in lines {
            if entries.len() == FILES_LIMIT {
                return Err("it names too many files".into());
            }
            let entry = parse_line(line).ok_or_else(|| format!("line {} is no file", index + 1))?;
            entries.push(entry);
        }
        Ok(Self::sorted(entries))
    }

    /// The list a mod carries: `None` without a `mod-files.txt`, an error
    /// when the file there cannot be read as one.
    fn read(dir: &Path) -> Option<Result<Self, String>> {
        let path = crate::util::case_insensitive_path::find(dir, LIST_NAME)?;
        let small_file = file_system::metadata(&path)
            .is_ok_and(|file| file.is_file() && file.len() <= LIST_LIMIT);
        if !small_file {
            return Some(Err("it is no file, or too large".into()));
        }
        let read = file_system::read(&path)
            .map_err(|error| error.to_string())
            .and_then(|bytes| String::from_utf8(bytes).map_err(|_| "it is not UTF-8 text".into()))
            .and_then(|text| Self::parse(&text));
        Some(read)
    }

    /// Write the list as `dir`'s `mod-files.txt`, over the one that is there.
    /// A link or a folder of that name is left alone: a mod from elsewhere
    /// must not aim the write at a file outside its folder.
    pub(crate) fn write(&self, dir: &Path) -> std::io::Result<PathBuf> {
        let text = self
            .text()
            .ok_or_else(|| std::io::Error::other("a file's name cannot be written in the list"))?;
        let path = crate::util::case_insensitive_path::find(dir, LIST_NAME)
            .unwrap_or_else(|| dir.join(LIST_NAME));
        if std::fs::symlink_metadata(&path).is_ok_and(|there| !there.is_file()) {
            return Err(std::io::Error::other("mod-files.txt is not a plain file"));
        }
        file_system::write(&path, text)?;
        Ok(path)
    }

    /// Where the folder's files, `found`, part from this list. Files count as
    /// in the pre-match check: by key and by what they hold, so another
    /// spelling of a name is no difference. Names outside ASCII share one key
    /// ([`name_key`]): among those in one folder a renamed file reads as the
    /// same file, and a new one beside a lost one as one changed file.
    fn differences(&self, found: &Self) -> Differences {
        let mut out = Differences::default();
        let (mut listed, mut found) = (self.entries.as_slice(), found.entries.as_slice());
        loop {
            let key = match (listed.first(), found.first()) {
                (Some(a), Some(b)) => (&a.key).min(&b.key),
                (Some(only), None) | (None, Some(only)) => &only.key,
                (None, None) => break,
            };
            let under_key = |entries: &[Entry]| {
                entries
                    .iter()
                    .position(|entry| &entry.key != key)
                    .unwrap_or(entries.len())
            };
            let (listed_here, listed_rest) = listed.split_at(under_key(listed));
            let (found_here, found_rest) = found.split_at(under_key(found));
            let (only_listed, only_found) = unmatched(listed_here, found_here);
            let pairs = only_listed.len().min(only_found.len());
            let paths = |entries: &[&Entry]| -> Vec<String> {
                entries.iter().map(|entry| entry.path.clone()).collect()
            };
            out.changed.extend(paths(&only_found[..pairs]));
            out.missing.extend(paths(&only_listed[pairs..]));
            out.extra.extend(paths(&only_found[pairs..]));
            (listed, found) = (listed_rest, found_rest);
        }
        out
    }
}

/// Of two runs of files under one key, each in the order of what they hold:
/// the files of each that the other has no equal for.
fn unmatched<'a>(a: &'a [Entry], b: &'a [Entry]) -> (Vec<&'a Entry>, Vec<&'a Entry>) {
    let (mut only_a, mut only_b) = (Vec::new(), Vec::new());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].digest.cmp(&b[j].digest) {
            std::cmp::Ordering::Equal => (i, j) = (i + 1, j + 1),
            std::cmp::Ordering::Less => {
                only_a.push(&a[i]);
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                only_b.push(&b[j]);
                j += 1;
            }
        }
    }
    only_a.extend(&a[i..]);
    only_b.extend(&b[j..]);
    (only_a, only_b)
}

/// Whether a path can stand in a line of the list and be read back as a path
/// below the mod's folder: parts with `/` between them, none empty, `.` or
/// `..`, and none with a `\`, a `:` or a control character. Windows can hold
/// no file of such a name either.
fn listable(path: &str) -> bool {
    path.split('/').all(|part| {
        !matches!(part, "" | "." | "..")
            && !part.contains(['\\', ':'])
            && !part.chars().any(char::is_control)
    })
}

/// One line of the list: 64 hex digits, the size in bytes and the path, a
/// space between them.
fn parse_line(line: &str) -> Option<Entry> {
    let (hex, rest) = line.split_once(' ')?;
    let (size, path) = rest.split_once(' ')?;
    if hex.len() != 64 || !hex.is_ascii() || size.len() > 20 || !listable(path) {
        return None;
    }
    let mut digest = [0; 32];
    for (byte, pair) in digest.iter_mut().zip(hex.as_bytes().chunks(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    if !size.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(Entry {
        key: path.split('/').map(name_key).collect::<Vec<_>>().join("/"),
        path: path.to_owned(),
        size: size.parse().ok()?,
        digest,
    })
}

/// Every file in `dir` and below, to `depth` folders down: its path below the
/// mod's folder as spelled and as it counts (`prefix`, both with `/` between
/// the parts), and where it is. Links are followed, as the game follows them.
fn collect_files(
    dir: &Path,
    prefix: (&str, &str),
    depth: usize,
    files: &mut Vec<(String, String, PathBuf)>,
) -> std::io::Result<()> {
    for entry in file_system::read_dir(dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().map(|name| name.to_string_lossy()) else {
            continue;
        };
        if crate::app::mods::left_by_a_system(&name) {
            continue;
        }
        let spelled = format!("{}{name}", prefix.0);
        let key = format!("{}{}", prefix.1, name_key(&name));
        let Ok(kind) = file_system::metadata(&path) else {
            continue;
        };
        if kind.is_dir() {
            // An HD art pack (`render::hd_art`, docs/hd-art.md): tens of
            // thousands of pictures only the screen draws, which players need
            // not share and the check need not read.
            if prefix.1.is_empty() && key == HD_ART_FOLDER {
                continue;
            }
            if depth > 0 {
                let below = (format!("{spelled}/"), format!("{key}/"));
                collect_files(&path, (&below.0, &below.1), depth - 1, files)?;
            }
        } else if kind.is_file() {
            if files.len() == FILES_LIMIT {
                return Err(std::io::Error::other("the mod has too many files"));
            }
            files.push((spelled, key, path));
        }
    }
    Ok(())
}

/// How a file's or folder's name counts in the digest: without ASCII case,
/// as the game finds files. A name with a letter outside ASCII counts as one
/// fixed mark, so only its place and what the file holds are compared:
/// systems spell such names differently (macOS can keep `é` as `e` and an
/// accent), and the same mod must give the same digest on each.
fn name_key(name: &str) -> String {
    if name.is_ascii() {
        name.to_ascii_uppercase()
    } else {
        "\u{fffd}".to_owned()
    }
}

/// A file's size and the SHA-256 of what it holds, read a piece at a time: a
/// mod's archives can be large.
fn file_digest(path: &Path) -> std::io::Result<(u64, [u8; 32])> {
    let mut digest = crate::util::sha256::Sha256::new();
    // The browser holds a mod's files in memory: hashed where they lie.
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    {
        let bytes = file_system::read_shared(path)?;
        digest.update(&bytes);
        Ok((bytes.len() as u64, digest.finalize()))
    }
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    {
        use std::io::Read;
        let mut file = std::fs::File::open(path)?;
        let mut piece = vec![0; 1 << 16];
        let mut size = 0;
        loop {
            match file.read(&mut piece)? {
                0 => return Ok((size, digest.finalize())),
                read => {
                    size += read as u64;
                    digest.update(&piece[..read]);
                }
            }
        }
    }
}

/// What a mod's list says of its folder.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Against {
    /// The mod carries no list.
    NoList,
    /// Its `mod-files.txt` cannot be read as a list.
    BadList,
    /// Every file is as listed, and there is no other.
    Same,
    Differs(Differences),
}

/// A mod's files as they are, and what its list says of them.
#[derive(Debug)]
pub(crate) struct Checked {
    pub(crate) found: FileList,
    pub(crate) against: Against,
}

/// Read the files of the mod in `dir` and hold them against its list. Every
/// path that differs goes to the log, which has room the game's boxes lack.
fn check(dir: &Path, stop: &AtomicBool) -> std::io::Result<Checked> {
    let found = FileList::scan(dir, stop)?;
    let against = match FileList::read(dir) {
        None => Against::NoList,
        Some(Err(reason)) => {
            log::warn!(
                "{}: not a file list: {reason}",
                dir.join(LIST_NAME).display()
            );
            Against::BadList
        }
        Some(Ok(listed)) => {
            let differences = listed.differences(&found);
            for (what, paths) in [
                ("changed", &differences.changed),
                ("missing", &differences.missing),
                ("not in the list", &differences.extra),
            ] {
                for path in paths {
                    log::info!("{}: {what}: {path}", dir.display());
                }
            }
            if differences.is_empty() {
                Against::Same
            } else {
                Against::Differs(differences)
            }
        }
    };
    Ok(Checked { found, against })
}

/// The Mods page's Check Files, from its button to its last box.
pub(crate) struct PageCheck {
    /// The mod's name as the page shows it.
    pub(crate) title: String,
    pub(crate) dir: PathBuf,
    pub(crate) step: Step,
}

pub(crate) enum Step {
    /// The files are being read on a thread of their own: a mod's archives
    /// take a moment. Setting `stop` ends the reading at the next file.
    Reading {
        stop: Arc<AtomicBool>,
        result: Receiver<std::io::Result<Checked>>,
    },
    /// The folder's files as read, which the open box offers to write as the
    /// mod's list.
    Offered(FileList),
}

/// Start reading the files of the mod in `dir`.
pub(crate) fn start(title: String, dir: PathBuf) -> std::io::Result<PageCheck> {
    let stop = Arc::new(AtomicBool::new(false));
    let (send, result) = channel();
    let (thread_dir, thread_stop) = (dir.clone(), Arc::clone(&stop));
    std::thread::Builder::new()
        .name("mod-check".into())
        .spawn(move || {
            // The page may have gone; then nobody waits for the answer.
            let checked =
                crate::util::mod_package::ModFolder::open(&thread_dir, Some(&thread_stop))
                    .and_then(|dir| check(&dir, &thread_stop));
            let _ = send.send(checked);
        })?;
    Ok(PageCheck {
        title,
        dir,
        step: Step::Reading { stop, result },
    })
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    /// An empty folder of the test's own, and a way to put files in it.
    fn folder(label: &str) -> (PathBuf, impl Fn(&str, &[u8])) {
        let root =
            std::env::temp_dir().join(format!("vera-mod-files-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let at = root.clone();
        let write = move |path: &str, bytes: &[u8]| {
            let path = at.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        };
        (root, write)
    }

    fn checked(dir: &Path) -> Checked {
        check(dir, &AtomicBool::new(false)).unwrap()
    }

    #[test]
    fn a_mods_files_digest_follows_what_the_files_hold_not_their_spelling_or_the_mods_name() {
        let (root, write) = folder("digest");
        write("One/rulesmd.ini", b"[General]\n");
        write("One/Maps/frozen.yrm", b"map");
        write("One/mod.ini", b"[Mod]\nName=One\n");
        // Another name, other spellings, another account of itself, a file
        // a system left, a sidebar layout and a file list: the same mod to a
        // match.
        write("Two/RULESMD.INI", b"[General]\n");
        write("Two/maps/Frozen.YRM", b"map");
        write("Two/mod.ini", b"[Mod]\nName=Two\nAbout=A copy.\n");
        write("Two/.DS_Store", b"x");
        write("Two/veraui.ini", b"[Layouts]\n");
        write("Two/Mod-Files.txt", b"anything");
        let digest = |folder: &str| checked(&root.join(folder)).found.digest();
        assert_eq!(digest("One"), digest("Two"));

        // A name outside ASCII counts by its place and what it holds: a Mac
        // can keep the same name in other bytes. A link to nothing is no file.
        write("One/Maps/caf\u{e9}.yrm", b"second map");
        write("Two/maps/cafe\u{301}.yrm", b"second map");
        #[cfg(unix)]
        std::os::unix::fs::symlink("nowhere", root.join("Two").join("gone.shp")).unwrap();
        assert_eq!(digest("One"), digest("Two"));

        // What a file below the folder holds counts, and so does one more file.
        write("Two/maps/Frozen.YRM", b"mop");
        assert_ne!(digest("One"), digest("Two"));
        write("Two/maps/Frozen.YRM", b"map");
        write("Two/tank.vxl", b"");
        assert_ne!(digest("One"), digest("Two"));
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Players of one mod need not share what only shows, plays or tells:
    /// a text table, a font, a movie, a picture only a screen draws, the
    /// pointer and a document, wherever in the folder. The mod's list still
    /// names them, and a sound, a palette or another shape still counts.
    #[test]
    fn a_mods_digest_leaves_out_what_only_shows_plays_or_tells() {
        let (root, write) = folder("shown");
        for folder in ["One", "Two"] {
            write(&format!("{folder}/rulesmd.ini"), b"[General]\n");
            write(&format!("{folder}/ra2md.csf"), folder.as_bytes());
        }
        for path in [
            "game.fnt",
            "intro.bik",
            "Movies/ending.VQA",
            "glsl.pcx",
            "Resources/logo.png",
            "Mouse.sha",
            "Readme.TXT",
            "debug.log",
        ] {
            write(&format!("Two/{path}"), b"shown");
        }
        let found = |folder: &str| checked(&root.join(folder)).found;
        assert_eq!(found("One").digest(), found("Two").digest());
        assert_eq!(found("One").code(), found("Two").code());
        assert_eq!((found("One").len(), found("Two").len()), (2, 10));

        for path in ["Two/theme.wav", "Two/unittem.pal", "Two/tank.sha"] {
            write(path, b"read");
            assert_ne!(found("One").digest(), found("Two").digest(), "{path}");
            std::fs::remove_file(root.join(path)).unwrap();
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    /// An HD art pack in a mod's folder is neither compared nor listed:
    /// players with and without it play together, and its pictures are not
    /// read before a match.
    #[test]
    fn an_hd_art_pack_is_neither_compared_nor_listed() {
        let (root, write) = folder("hd-art");
        write(
            "One/rulesmd.ini",
            b"[General]
",
        );
        write(
            "Two/rulesmd.ini",
            b"[General]
",
        );
        write("Two/hd_art/pack/0123456789abcdef.png", b"picture");
        write("Two/HD_ART/pack/terrain/fedcba9876543210.png", b"picture");
        let found = |folder: &str| checked(&root.join(folder)).found;
        assert_eq!(found("One").digest(), found("Two").digest());
        assert_eq!((found("One").len(), found("Two").len()), (1, 1));
        // A folder of that name below the mod's own still counts.
        write("Two/Art/hd_art/tank.vxl", b"model");
        assert_ne!(found("One").digest(), found("Two").digest());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_mods_list_names_the_changed_the_missing_and_the_extra_file() {
        let (root, write) = folder("list");
        write("rulesmd.ini", b"[General]\n");
        write("Maps/frozen.yrm", b"map");
        write("Art/tank.vxl", b"voxel");
        write("mod.ini", b"[Mod]\nName=One\n");
        let made = checked(&root);
        assert_eq!(made.against, Against::NoList);
        assert_eq!(made.found.len(), 3);
        let list = made.found.write(&root).unwrap();
        assert_eq!(list, root.join(LIST_NAME));

        // The copy its maker listed, a file respelled and the mod's account
        // of itself rewritten: nothing differs, and the code is the same.
        std::fs::rename(root.join("rulesmd.ini"), root.join("RulesMD.ini")).unwrap();
        write("mod.ini", b"[Mod]\nName=Other\n");
        let same = checked(&root);
        assert_eq!(same.against, Against::Same);
        assert_eq!(same.found.code(), made.found.code());
        assert_eq!(same.found.code().len(), 9);

        write("Maps/frozen.yrm", b"mop");
        std::fs::remove_file(root.join("Art").join("tank.vxl")).unwrap();
        write("notes.txt", b"mine");
        assert_eq!(
            checked(&root).against,
            Against::Differs(Differences {
                changed: vec!["Maps/frozen.yrm".into()],
                missing: vec!["Art/tank.vxl".into()],
                extra: vec!["notes.txt".into()],
            })
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_file_that_is_no_list_is_refused_and_read_as_none() {
        let (root, write) = folder("bad");
        write("rulesmd.ini", b"[General]\n");
        let digest = "0".repeat(64);
        for bad in [
            String::new(),
            "rulesmd.ini".into(),
            "VERA20K mod files 2\n".into(),
            format!("{HEADER}\n{digest} 10 ../rulesmd.ini\n"),
            format!("{HEADER}\n{digest} 10 /etc/passwd\n"),
            format!("{HEADER}\n{digest} 10 C:\\rulesmd.ini\n"),
            format!("{HEADER}\n{digest} -1 rulesmd.ini\n"),
            format!("{HEADER}\n{} 10 rulesmd.ini\n", "g".repeat(64)),
            format!("{HEADER}\n{}\n", "\u{e9}".repeat(40)),
        ] {
            write(LIST_NAME, bad.as_bytes());
            assert_eq!(checked(&root).against, Against::BadList, "{bad:?}");
        }
        write(LIST_NAME, &[0xff, 0xfe, 0x00]);
        assert_eq!(checked(&root).against, Against::BadList);
        // A list that is a link is never written through.
        #[cfg(unix)]
        {
            let (outside, list) = (root.join("Art").join("kept.txt"), root.join(LIST_NAME));
            write("Art/kept.txt", b"kept");
            std::fs::remove_file(&list).unwrap();
            std::os::unix::fs::symlink(&outside, &list).unwrap();
            assert!(checked(&root).found.write(&root).is_err());
            assert_eq!(std::fs::read(&outside).unwrap(), b"kept");
            std::fs::remove_file(&list).unwrap();
            std::fs::remove_file(&outside).unwrap();
        }
        // Notepad's form of a good list reads: a mark first, CR LF ends.
        let good = FileList::scan(&root, &AtomicBool::new(false)).unwrap();
        let text = good.text().unwrap().replace('\n', "\r\n");
        write(LIST_NAME, format!("\u{feff}{text}").as_bytes());
        assert_eq!(checked(&root).against, Against::Same);
        std::fs::remove_dir_all(root).unwrap();
    }
}
