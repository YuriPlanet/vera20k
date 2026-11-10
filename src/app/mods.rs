//! Mods: a folder of a player's own files, read on top of the game folder.
//!
//! VERA-only. gamemd has one folder, and a mod is copied into it. Here each
//! mod is a subfolder of `Mods` in the settings folder (docs/modding.md), and
//! the one in use reads as if its files had been copied on top of the game
//! folder ([`AssetManager::with_mod`], `map::source::set_mod_dir`). With no
//! mod in use nothing here changes what the game reads.
//!
//! This module decides which mod a launch uses: the `--mod <folder>` or
//! `--no-mod` switch, else config.toml's `[paths] mod`. A choice that cannot
//! be used never stops the game: the launch plays the original and the main
//! menu says why.
//!
//! A mod that stops start-up would leave the player no menu to leave it from.
//! So a start with a mod writes the mod's name to `mod-start.txt` in the
//! settings folder and keeps the file locked. It removes the file once the
//! main menu has been at rest a moment, or the game is left in the ordinary
//! way with no error card up. A start that finds the name there with no
//! running copy holding the file plays without that mod, once.
//!
//! [`AssetManager::with_mod`]: crate::assets::asset_manager::AssetManager::with_mod

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

use crate::app::state::AppState;
use crate::util::config::PathsConfig;
use crate::util::file_system;
use crate::util::time::{Duration, Instant};

const USE_SWITCH: &str = "--mod";
const NO_MOD_SWITCH: &str = "--no-mod";
const MOD_INI: &str = "mod.ini";
const README: &str = "README.txt";
const START_RECORD: &str = "mod-start.txt";
/// The most of `mod.ini` that is read: far more than its few lines take.
const MOD_INI_LIMIT: u64 = 64 * 1024;
/// How long the main menu has been at rest when a start with a mod counts as
/// good: its first frames, with the mod's own menu art and movie, have drawn.
const MENU_SETTLE: Duration = Duration::from_secs(2);

/// A launch switch's choice, which takes the place of config.toml's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModSwitch {
    /// `--mod <folder>`.
    Use(String),
    /// `--no-mod`: the original game.
    NoMod,
}

impl ModSwitch {
    /// The arguments that give a run this program starts the same choice.
    fn args(&self) -> Vec<OsString> {
        match self {
            Self::Use(folder) => vec![USE_SWITCH.into(), folder.into()],
            Self::NoMod => vec![NO_MOD_SWITCH.into()],
        }
    }
}

/// Take `--mod <folder>` or `--no-mod` out of the process arguments, wherever
/// it stands. The retail switch table matches by substring (`-CD`) and must
/// never see a folder's name, and the other modes' parsers take exact lists.
pub fn take_switch(args: Vec<OsString>) -> Result<(Option<ModSwitch>, Vec<OsString>)> {
    let mut switch = None;
    let mut rest = Vec::with_capacity(args.len());
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let found = if arg == USE_SWITCH {
            let folder = args
                .next()
                .context("--mod needs the name of a folder in Mods")?
                .into_string()
                .map_err(|_| anyhow::anyhow!("the --mod folder's name is not Unicode text"))?;
            ModSwitch::Use(folder)
        } else if arg == NO_MOD_SWITCH {
            ModSwitch::NoMod
        } else {
            rest.push(arg);
            continue;
        };
        ensure!(
            switch.replace(found).is_none(),
            "give --mod or --no-mod once"
        );
    }
    Ok((switch, rest))
}

/// The mod a launch reads on top of the game folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveMod {
    /// Its folder's name in `Mods`, as the file system spells it.
    pub(crate) folder: String,
    pub(crate) dir: PathBuf,
    /// What the player is shown: `mod.ini`'s `Name=` and `Version=`, else the
    /// folder's name.
    pub(crate) title: String,
}

/// How a launch's mod choice came out; the app keeps it for the run.
#[derive(Default)]
pub(crate) struct Mods {
    pub(crate) active: Option<ActiveMod>,
    /// Why the mod asked for is not in use, or did not load: shown once the
    /// main menu is at rest.
    notice: Option<String>,
    /// `mod-start.txt`, held while this run's start with a mod has yet to
    /// pass or fail. Never held by a run the game starts for itself (a map
    /// picture, a capture), which leaves the record to the player's own run.
    record: Option<StartRecord>,
    /// When [`poll`] first saw the main menu at rest, this time it is.
    menu_rest_since: Option<Instant>,
}

impl Mods {
    /// The folder the game files and the maps are read from first.
    pub(crate) fn dir(&self) -> Option<&Path> {
        self.active.as_ref().map(|active| active.dir.as_path())
    }

    /// The switch that gives a run this program starts the same mod, or none:
    /// such a run must not choose for itself from config.toml.
    pub(crate) fn child_args(&self) -> Vec<OsString> {
        self.active
            .as_ref()
            .map_or(ModSwitch::NoMod, |active| {
                ModSwitch::Use(active.folder.clone())
            })
            .args()
    }

    /// Why the mod this launch asked for is not in use, while the player has
    /// not been told. A launch by another program has no menu to say it in
    /// and ends on it (`App::start_spawn_session`).
    pub(crate) fn refusal(&self) -> Option<&str> {
        self.notice.as_deref()
    }
}

/// What `mod.ini` says. VERA's own file, in UTF-8: `[Mod]` with `Name=`,
/// `Version=` and `Needs=`, every key optional, as the file is. Not read with
/// gamemd's INI reader, which takes bytes as Latin-1 and drops a last line
/// that has no line end.
#[derive(Debug, Default, PartialEq, Eq)]
struct ModInfo {
    name: Option<String>,
    version: Option<String>,
    /// The lowest VERA20K version that can play the mod.
    needs: Option<String>,
}

impl ModInfo {
    /// The mod's own account of itself, when its folder has a `mod.ini` that
    /// is a file of a few lines; anything else there says nothing.
    fn read(dir: &Path) -> Self {
        let Some(path) = crate::util::case_insensitive_path::find(dir, MOD_INI) else {
            return Self::default();
        };
        let small_file = file_system::metadata(&path)
            .is_ok_and(|file| file.is_file() && file.len() <= MOD_INI_LIMIT);
        if !small_file {
            log::warn!("{} is not a small file; not read", path.display());
            return Self::default();
        }
        file_system::read(&path)
            .map(|bytes| Self::parse(&String::from_utf8_lossy(&bytes)))
            .unwrap_or_default()
    }

    fn parse(text: &str) -> Self {
        let mut info = Self::default();
        let mut in_mod = false;
        for line in text.trim_start_matches('\u{feff}').lines() {
            // As in the game's own INI files, `;` starts a comment.
            let line = line.split(';').next().unwrap_or_default().trim();
            if let Some(header) = line.strip_prefix('[') {
                in_mod = header
                    .strip_suffix(']')
                    .is_some_and(|name| name.trim().eq_ignore_ascii_case("Mod"));
                continue;
            }
            if !in_mod {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let slot = match key.trim().to_ascii_lowercase().as_str() {
                "name" => &mut info.name,
                "version" => &mut info.version,
                "needs" => &mut info.needs,
                _ => continue,
            };
            *slot = Some(value.trim().to_owned()).filter(|value| !value.is_empty());
        }
        // A file saved in another encoding would show as boxes; the folder's
        // name, which the file system gives as Unicode, shows as it is.
        for shown in [&mut info.name, &mut info.version] {
            if shown.as_ref().is_some_and(|text| text.contains('\u{fffd}')) {
                *shown = None;
            }
        }
        info
    }

    fn title(&self, folder: &str) -> String {
        match (self.name.as_deref().unwrap_or(folder), &self.version) {
            (name, Some(version)) => format!("{name} {version}"),
            (name, None) => name.to_owned(),
        }
    }
}

/// `1`, `1.2` or `1.2.3` as three numbers, absent ones 0; anything else,
/// such as a later scheme this build does not know, is `None`.
fn version_parts(text: &str) -> Option<[u32; 3]> {
    let mut parts = [0; 3];
    let mut given = text.trim().split('.');
    for part in &mut parts {
        match given.next() {
            Some(digits) if digits.len() <= 9 && !digits.is_empty() => {
                *part = digits
                    .bytes()
                    .all(|byte| byte.is_ascii_digit())
                    .then(|| digits.parse().ok())
                    .flatten()?;
            }
            Some(_) => return None,
            None => break,
        }
    }
    given.next().is_none().then_some(parts)
}

/// Whether this build, `current`, can play a mod that needs `needs`. A build
/// whose own version does not read as three numbers refuses nothing.
fn version_serves(current: &str, needs: &str) -> bool {
    match (version_parts(current), version_parts(needs)) {
        (Some(current), Some(needs)) => current >= needs,
        (None, _) => true,
        (Some(_), None) => false,
    }
}

/// The folder in `mods_dir` that `name` names, whatever its case: its own
/// spelling and its path. A name that is not one plain folder name (empty, a
/// path, `..`) names nothing.
fn find_folder(mods_dir: &Path, name: &str) -> Option<(String, PathBuf)> {
    let mut components = Path::new(name).components();
    let plain = matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
        && !name.contains(['/', '\\']);
    if !plain {
        return None;
    }
    let dir = crate::util::case_insensitive_path::find(mods_dir, name)
        .filter(|path| file_system::is_dir(path))?;
    let folder = dir.file_name()?.to_str()?.to_owned();
    Some((folder, dir))
}

/// The browser build has no folder a player can put files in.
const SUPPORTED: bool = !cfg!(all(target_arch = "wasm32", target_os = "unknown"));

/// The mod that `wanted` names in `Mods`, or why it cannot be used, said in
/// the player's language. Reads the folder and its `mod.ini`; writes nothing.
fn resolve(paths: &PathsConfig, wanted: &str) -> Result<ActiveMod, String> {
    let mods_dir = paths.mods_dir();
    let Some((folder, dir)) = find_folder(&mods_dir, wanted) else {
        log::warn!("Mod {wanted:?} is not a folder in {}", mods_dir.display());
        return Err(crate::i18n::tr_fill(
            "VERA:ModNotFound",
            &[("mod", &wanted), ("folder", &mods_dir.display())],
        ));
    };
    let info = ModInfo::read(&dir);
    let title = info.title(&folder);
    let version = env!("CARGO_PKG_VERSION");
    if let Some(needs) = info
        .needs
        .as_deref()
        .filter(|needs| !version_serves(version, needs))
    {
        log::warn!("Mod {folder} needs VERA20k {needs}; this is {version}");
        return Err(crate::i18n::tr_fill(
            "VERA:ModNeedsNewer",
            &[("mod", &title), ("needs", &needs), ("version", &version)],
        ));
    }
    Ok(ActiveMod { folder, dir, title })
}

/// The folder a launch asks for: its switch's, else config.toml's where
/// `config_counts`.
fn wanted<'a>(
    paths: &'a PathsConfig,
    switch: Option<&'a ModSwitch>,
    config_counts: bool,
) -> Option<&'a str> {
    match switch {
        Some(ModSwitch::Use(folder)) => Some(folder.as_str()),
        Some(ModSwitch::NoMod) => None,
        None if config_counts => paths.mod_name(),
        None => None,
    }
}

/// The folder of the mod a launch with `switch` would read: for the launch
/// probe (`spawn_probe`), which reports what a launch reads without being
/// one, so it leaves `mod-start.txt` alone.
pub(crate) fn launch_dir(paths: &PathsConfig, switch: Option<&ModSwitch>) -> Option<PathBuf> {
    let wanted = wanted(paths, switch, true).filter(|_| SUPPORTED)?;
    resolve(paths, wanted).ok().map(|active| active.dir)
}

/// A run's hold on `mod-start.txt`: the file names the mod being started and
/// stays locked by the copy of the game that wrote it, so a later start can
/// tell a start that died (the name, held by nothing) from a copy that is
/// still starting beside it (the file, locked).
struct StartRecord {
    path: PathBuf,
    locked: std::fs::File,
}

impl StartRecord {
    /// The start has passed. The name goes while the file is still held, so
    /// no copy starting this instant reads it as a start that died.
    fn clear(self) {
        let _ = self.locked.set_len(0);
        drop(self.locked);
        let _ = file_system::remove_file(&self.path);
    }
}

/// What `mod-start.txt` says to a start with the mod in `folder`.
enum Claim {
    /// This run now holds the record.
    Held(StartRecord),
    /// Another running copy holds it: nothing has failed.
    Elsewhere,
    /// It named this mod and nothing held it: the start that wrote it died.
    /// The record is gone, so the next start tries the mod again.
    Died,
}

fn claim(path: &Path, folder: &str) -> std::io::Result<Claim> {
    use std::io::{Read, Seek, Write};
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    match file.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => return Ok(Claim::Elsewhere),
        Err(std::fs::TryLockError::Error(error)) => return Err(error),
    }
    // A record is one folder's name; anything longer is not one.
    let mut named = Vec::new();
    (&mut file).take(1024).read_to_end(&mut named)?;
    if String::from_utf8_lossy(&named).trim() == folder {
        drop(file);
        let _ = file_system::remove_file(path);
        return Ok(Claim::Died);
    }
    file.set_len(0)?;
    file.rewind()?;
    file.write_all(folder.as_bytes())?;
    file.flush()?;
    Ok(Claim::Held(StartRecord {
        path: path.to_owned(),
        locked: file,
    }))
}

/// Decide this launch's mod.
///
/// `hidden` is a run with a sealed set-up (a capture, a map picture): it
/// reads no choice from config.toml, as it reads no VERA.INI or VERARULES.INI,
/// and uses a mod only when a switch names one.
pub(crate) fn begin(paths: &PathsConfig, switch: Option<&ModSwitch>, hidden: bool) -> Mods {
    let mut mods = Mods::default();
    if !SUPPORTED {
        return mods;
    }
    if !hidden {
        prepare_folder(&paths.mods_dir());
    }
    let Some(wanted) = wanted(paths, switch, !hidden) else {
        return mods;
    };
    let active = match resolve(paths, wanted) {
        Ok(active) => active,
        Err(notice) => {
            mods.notice = Some(notice);
            return mods;
        }
    };
    if !hidden {
        let record = paths.settings_dir().join(START_RECORD);
        match claim(&record, &active.folder) {
            Ok(Claim::Held(held)) => mods.record = Some(held),
            Ok(Claim::Elsewhere) => {}
            Ok(Claim::Died) => {
                log::warn!(
                    "Mod {} did not finish its last start; playing without it once",
                    active.folder
                );
                mods.notice = Some(crate::i18n::tr_fill(
                    "VERA:ModSkipped",
                    &[("mod", &active.title)],
                ));
                return mods;
            }
            // Without the record the mod still plays; only the way out of a
            // mod that stops start-up is missing.
            Err(error) => log::warn!("Could not keep {}: {error}", record.display()),
        }
    }
    log::info!("Mod in use: {} ({})", active.title, active.dir.display());
    mods.active = Some(active);
    mods
}

/// What a point of the run makes of a start with a mod.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// It passed: the record goes.
    Passed,
    /// It failed: the record stays for the next start to find.
    Failed,
    /// Not known yet.
    Open,
}

/// Loading is over. A start whose mod did not load has failed. One that goes
/// straight into a match (`-SPAWN`, a replay, the developer's quick play) has
/// no menu to wait for and has passed; any other waits for the menu, where
/// the mod's own menu art and movie have yet to draw.
fn after_loading(error_card: bool, rules_loaded: bool, straight_to_match: bool) -> Verdict {
    if error_card || !rules_loaded {
        Verdict::Failed
    } else if straight_to_match {
        Verdict::Passed
    } else {
        Verdict::Open
    }
}

/// The game is left in the ordinary way. A start still on its way to the menu
/// did not fail, the player closed it; one that ended on the error card did,
/// whenever the card came up, and Quit is the card's only way out.
fn on_leaving(error_card: bool) -> Verdict {
    if error_card {
        Verdict::Failed
    } else {
        Verdict::Passed
    }
}

/// Loading is over ([`after_loading`]); `external_launch` is `-SPAWN` or a
/// replay. A mod that did not load stays on record, and the player is told
/// what to do.
pub(crate) fn finish_start(state: &mut AppState, external_launch: bool) {
    let mods = &state.frontend.mods;
    let Some(active) = mods.active.as_ref().filter(|_| mods.record.is_some()) else {
        return;
    };
    let error_card = state.frontend.main_menu_shell_error.is_some();
    let straight_to_match =
        external_launch || state.frontend.screen != crate::ui::game_screen::GameScreen::MainMenu;
    let rules_loaded = state.process_assets.has_native_rules();
    match after_loading(error_card, rules_loaded, straight_to_match) {
        Verdict::Passed => started(state),
        Verdict::Open => {}
        Verdict::Failed => {
            log::warn!(
                "Mod {} did not load; the next start is without it",
                active.folder
            );
            // The error card says it for itself (`App::render_shell_error`).
            if !error_card {
                state.frontend.mods.notice = Some(start_failed_text(&active.title));
            }
            // Let go of the file and leave its name in it.
            state.frontend.mods.record = None;
        }
    }
}

/// The start with a mod has passed: its record goes.
fn started(state: &mut AppState) {
    if let Some(record) = state.frontend.mods.record.take() {
        record.clear();
    }
}

/// The game is left in the ordinary way ([`on_leaving`]).
pub(crate) fn left(state: &mut AppState) {
    if on_leaving(state.frontend.main_menu_shell_error.is_some()) == Verdict::Passed {
        started(state);
    }
}

/// What a start that failed with the mod `title` tells the player to do.
pub(crate) fn start_failed_text(title: &str) -> String {
    crate::i18n::tr_fill("VERA:ModStartFailed", &[("mod", &title)])
}

/// Once the main menu has been at rest for [`MENU_SETTLE`]: a start with a
/// mod has passed, and a mod that is not in use is explained. Returns when to
/// look again while that is still to come.
pub(crate) fn poll(state: &mut AppState) -> Option<Instant> {
    let mods = &state.frontend.mods;
    if mods.record.is_none() && mods.notice.is_none() {
        return None;
    }
    if !state.main_menu_at_rest() {
        state.frontend.mods.menu_rest_since = None;
        return None;
    }
    let now = Instant::now();
    let due = *state.frontend.mods.menu_rest_since.get_or_insert(now) + MENU_SETTLE;
    if now < due {
        return Some(due);
    }
    started(state);
    if let Some(text) = state.frontend.mods.notice.take() {
        crate::app::App::open_shell_notice(state, text);
    }
    None
}

/// Make `Mods` and its README. The folder is the player's: nothing else in
/// it is touched, and a failure only goes to the log.
fn prepare_folder(mods_dir: &Path) {
    let mut text = crate::i18n::tr("VERA:ModsFolderReadme");
    if cfg!(windows) {
        // Notepad before Windows 10 1809 needs both to show the text.
        text = format!("\u{feff}{}", text.replace('\n', "\r\n"));
    }
    let readme = mods_dir.join(README);
    let made = file_system::create_dir_all(mods_dir).and_then(|()| {
        if file_system::read(&readme).is_ok_and(|kept| kept == text.as_bytes()) {
            Ok(())
        } else {
            file_system::write(&readme, &text)
        }
    });
    if let Err(error) = made {
        log::warn!("Could not prepare {}: {error}", mods_dir.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    /// A settings folder with `Mods/Winter War` in it.
    fn settings(label: &str) -> (PathBuf, PathsConfig) {
        let root = std::env::temp_dir().join(format!("vera-mods-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Mods").join("Winter War")).unwrap();
        let config = format!(
            "[paths]\nra2_dir = '{}'\nsettings_dir = '{}'\nmod = 'winter war'\n",
            root.join("game").display(),
            root.display()
        );
        let path = root.join("config.toml");
        std::fs::write(&path, config).unwrap();
        let paths = crate::util::config::GameConfig::load_from(&root, &root.join("vera20k"))
            .unwrap()
            .paths;
        (root, paths)
    }

    #[test]
    fn the_switch_leaves_the_other_arguments_and_keeps_its_value_from_them() {
        let (switch, rest) = take_switch(os(&["-WIN", "--mod", "my-cd-mod", "-SPAWN"])).unwrap();
        assert_eq!(switch, Some(ModSwitch::Use("my-cd-mod".into())));
        assert_eq!(rest, os(&["-WIN", "-SPAWN"]));
        assert_eq!(
            take_switch(os(&["--no-mod"])).unwrap(),
            (Some(ModSwitch::NoMod), Vec::new())
        );
        assert!(take_switch(os(&["--mod"])).is_err());
        assert!(take_switch(os(&["--mod", "a", "--no-mod"])).is_err());
    }

    #[test]
    fn a_mod_whose_last_start_died_is_skipped_once() {
        let (root, paths) = settings("record");
        let record = root.join(START_RECORD);

        let first = begin(&paths, None, false);
        assert_eq!(first.active.as_ref().unwrap().folder, "Winter War");
        assert_eq!(
            first.dir(),
            Some(root.join("Mods").join("Winter War").as_path())
        );
        assert!(first.record.is_some() && root.join("Mods").join(README).is_file());

        // A second copy started beside it has no failed start to go by, and
        // a start without a mod leaves the record to the copy that holds it.
        let beside = begin(&paths, None, false);
        assert!(beside.active.is_some() && beside.record.is_none() && beside.notice.is_none());
        assert!(
            begin(&paths, Some(&ModSwitch::NoMod), false)
                .active
                .is_none()
        );

        // The first copy dies before the menu: the next start is without the
        // mod, once.
        drop((first, beside));
        assert_eq!(std::fs::read_to_string(&record).unwrap(), "Winter War");
        let second = begin(&paths, None, false);
        assert!(second.active.is_none() && second.notice.is_some());
        assert!(!record.exists());
        let third = begin(&paths, None, false);
        assert!(third.active.is_some());

        // A run the game starts for itself takes no choice from config.toml
        // and never holds the record.
        assert!(begin(&paths, None, true).active.is_none());
        let told = begin(&paths, Some(&ModSwitch::Use("WINTER WAR".into())), true);
        assert_eq!(told.child_args(), os(&["--mod", "Winter War"]));
        assert!(told.record.is_none());

        // A start that passed leaves nothing behind.
        third.record.unwrap().clear();
        assert!(!record.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_start_that_ends_on_the_error_card_stays_on_record() {
        // Loaded, then the card at the first menu frame, then Quit.
        assert_eq!(after_loading(false, true, false), Verdict::Open);
        assert_eq!(on_leaving(true), Verdict::Failed);
        // Closed on the way to a menu that showed no card.
        assert_eq!(on_leaving(false), Verdict::Passed);
        // A launch straight into a match passes once it has loaded, unless
        // loading failed.
        assert_eq!(after_loading(false, true, true), Verdict::Passed);
        assert_eq!(after_loading(true, true, true), Verdict::Failed);
        assert_eq!(after_loading(false, false, false), Verdict::Failed);
    }

    #[test]
    fn a_missing_folder_or_a_newer_need_plays_the_original_and_says_so() {
        let (root, paths) = settings("refused");
        for name in ["Summer", "..", "Winter War/sub", ""] {
            let refused = begin(&paths, Some(&ModSwitch::Use(name.into())), false);
            assert!(
                refused.active.is_none() && refused.refusal().is_some(),
                "{name:?}"
            );
        }
        // No line end after the last line, and a name outside Latin-1.
        let ini = root.join("Mods").join("Winter War").join("MOD.INI");
        std::fs::write(&ini, "[mod]\nName=冬季战争\nVersion = 1.2\nNeeds=999999.0").unwrap();
        let newer = begin(&paths, None, false);
        assert!(newer.active.is_none() && newer.refusal().is_some());
        assert!(!root.join(START_RECORD).exists());

        std::fs::write(
            &ini,
            "[Mod]\nName=冬季战争\nVersion = 1.2\nNeeds=0.1 ; any\n",
        )
        .unwrap();
        let told = ModSwitch::Use("Winter War".into());
        assert_eq!(
            begin(&paths, Some(&told), true).active.unwrap().title,
            "冬季战争 1.2"
        );
        // A name saved in another encoding is not shown as boxes.
        std::fs::write(&ini, b"[Mod]\nName=\xb6\xac\xbc\xbe\nVersion=1.2\n").unwrap();
        assert_eq!(
            begin(&paths, Some(&told), true).active.unwrap().title,
            "Winter War 1.2"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_needed_version_compares_by_number_and_an_unreadable_one_refuses() {
        assert!(version_serves("0.2.10", "0.2.10"));
        assert!(version_serves("0.2.10", "0.2.9"));
        assert!(version_serves("0.3.0", "0.3"));
        assert!(!version_serves("0.2.10", "0.2.11"));
        assert!(!version_serves("0.2.10", "0.3"));
        for unreadable in ["", "0.2.x", "1.2.3.4", "0..1", "v1"] {
            assert!(!version_serves("0.2.10", unreadable), "{unreadable:?}");
        }
        assert!(version_serves("0.3.0-dev", "9.0"));
    }
}
