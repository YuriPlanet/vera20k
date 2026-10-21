//! Starting VERA20k from its macOS app bundle.
//!
//! Finder starts an app in `/`, and the bundle can be read-only: a mounted disk
//! image, or a downloaded copy that macOS runs from a randomized location (App
//! Translocation). A launch from `VERA20k.app` therefore works in
//! `~/Library/Application Support/VERA20k`, the way the Windows shortcuts start
//! the game in its install folder: config.toml, RA2MD.INI, logs, saves and
//! replays all live there.
//!
//! Until config.toml names a Yuri's Revenge folder, each launch sets one up
//! ([`ensure_game_folder`]). It first looks where Mac players keep the game,
//! inside a Wine prefix, and offers the folder it finds in one question; only
//! when there is none, or the player wants another, does the system folder
//! chooser open.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The data folder, below the home folder.
const DATA_FOLDER: &str = "Library/Application Support/VERA20k";

/// When this process runs from a macOS app bundle, make the app's data folder
/// the working directory and return true. Call it before anything opens a file
/// relative to the working directory, the log first.
pub fn enter_data_folder() -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    let in_bundle = std::env::current_exe().is_ok_and(|executable| in_app_bundle(&executable));
    let Some(home) = std::env::var_os("HOME").filter(|_| in_bundle) else {
        return false;
    };
    let folder = PathBuf::from(home).join(DATA_FOLDER);
    match std::fs::create_dir_all(&folder).and_then(|()| std::env::set_current_dir(&folder)) {
        Ok(()) => true,
        Err(error) => {
            // The logger does not exist yet.
            eprintln!("Cannot use the data folder {}: {error}", folder.display());
            false
        }
    }
}

/// Whether `executable` is `<name>.app/Contents/MacOS/<executable>`.
fn in_app_bundle(executable: &Path) -> bool {
    let mut folders = executable.ancestors().skip(1);
    let (Some(macos), Some(contents), Some(bundle)) =
        (folders.next(), folders.next(), folders.next())
    else {
        return false;
    };
    macos.file_name() == Some(OsStr::new("MacOS"))
        && contents.file_name() == Some(OsStr::new("Contents"))
        && bundle.extension() == Some(OsStr::new("app"))
}

#[cfg(target_os = "macos")]
pub use first_launch::ensure_game_folder;

/// Only the macOS app bundle sets up a game folder.
#[cfg(not(target_os = "macos"))]
pub fn ensure_game_folder() -> bool {
    true
}

/// The app bundle's game folder setup. Its questions run in the event loop's
/// first `resumed`, before any window exists: macOS brings forward only the app
/// it launched, once that app is running. A chooser shown earlier by a helper
/// process (`osascript`) stayed hidden behind the app the player was using.
#[cfg(target_os = "macos")]
mod first_launch {
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};

    use anyhow::{Context, Result};
    use objc2::MainThreadMarker;

    use crate::assets::asset_manager::holds_master_archives;
    use crate::util::config::{GameConfig, set_paths_key};

    /// A first launch starts config.toml from this, so the file documents
    /// every setting.
    const CONFIG_TEMPLATE: &str = include_str!("../../config.toml.example");
    /// How deep below a Wine prefix's `drive_c` the search looks. Yuri's
    /// Revenge sits five folders down in The First Decade and Steam installs.
    const SEARCH_DEPTH: usize = 6;
    /// The most folders the search reads in one prefix, so a large Steam
    /// library cannot stall a first launch.
    const SEARCH_LIMIT: usize = 20_000;
    /// Windows' own folders at the top of a prefix, which never hold the game.
    /// `users` also links to the player's Mac folders.
    const WINDOWS_FOLDERS: [&str; 3] = ["windows", "users", "programdata"];

    /// What the player said to a game folder the search found.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum Offer {
        Use,
        ChooseAnother,
        Quit,
    }

    /// The questions a first launch asks the player.
    pub(super) trait Player {
        /// Offer a folder the search found.
        fn offer(&mut self, folder: &Path) -> Result<Offer>;
        /// Let the player choose a folder, starting in `start`; `retry` after
        /// one without the masters. None means the player cancelled.
        fn choose(&mut self, start: Option<&Path>, retry: bool) -> Result<Option<PathBuf>>;
    }

    /// Make sure config.toml in the data folder names a Yuri's Revenge
    /// folder, asking until it does. Returns false when the player quits.
    /// Call it on the main thread from the event loop.
    pub fn ensure_game_folder() -> bool {
        let asked = (|| -> Result<bool> {
            let data_folder = std::env::current_dir()?;
            let executable = std::env::current_exe()?;
            let home = PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?);
            let mut dialogs = SystemDialogs::new()?;
            ensure_game_folder_with(
                &data_folder,
                &executable,
                || find_game_folders(&home, Path::new("/Applications")),
                &mut dialogs,
            )
        })();
        asked.unwrap_or_else(|error| {
            // The startup error screen still says which game files are missing.
            log::warn!("Could not set up the game folder: {error:#}");
            true
        })
    }

    pub(super) fn ensure_game_folder_with(
        data_folder: &Path,
        executable: &Path,
        find: impl FnOnce() -> Vec<PathBuf>,
        player: &mut impl Player,
    ) -> Result<bool> {
        // A config.toml that does not load is left to the startup error
        // screen, which names its problem.
        match GameConfig::load_from(data_folder, executable) {
            Ok(config) if !holds_master_archives(&config.paths.ra2_dir) => {}
            _ => return Ok(true),
        }
        let found = find();
        log::info!("Yuri's Revenge folders found: {found:?}");
        let offered = match found.first() {
            Some(folder) => match player.offer(folder)? {
                Offer::Use => Some(folder.clone()),
                Offer::ChooseAnother => None,
                Offer::Quit => return Ok(false),
            },
            None => None,
        };
        let folder = match offered {
            Some(folder) => folder,
            None => {
                // Beside the found folder, where any other copies are.
                let start = found.first().and_then(|folder| folder.parent());
                let mut retry = false;
                loop {
                    let Some(folder) = player.choose(start, retry)? else {
                        return Ok(false);
                    };
                    if holds_master_archives(&folder) {
                        break folder;
                    }
                    log::info!("No ra2md.mix and ra2.mix in {}", folder.display());
                    retry = true;
                }
            }
        };

        let ra2_dir = folder
            .to_str()
            .context("The game folder's path is not valid UTF-8")?;
        let path = data_folder.join("config.toml");
        let config = match std::fs::read_to_string(&path) {
            Ok(config) => set_paths_key(&config, "ra2_dir", ra2_dir),
            // RA2MD.INI goes beside config.toml rather than into the game folder.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => set_paths_key(
                &set_paths_key(CONFIG_TEMPLATE, "ra2_dir", ra2_dir),
                "settings_dir",
                ".",
            ),
            Err(error) => {
                return Err(error).with_context(|| format!("Failed to read {}", path.display()));
            }
        };
        std::fs::write(&path, config)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        log::info!("Game folder {ra2_dir} written to {}", path.display());
        Ok(true)
    }

    /// Yuri's Revenge folders inside the Wine prefixes Mac players use:
    /// wrapper apps from Porting Kit, Wineskin and Kegworks in `applications`
    /// or ~/Applications, CrossOver bottles and ~/.wine. None of these places
    /// makes macOS ask the player for file access, and the search follows no
    /// link out of them. Sorted, so the offer is stable.
    pub(super) fn find_game_folders(home: &Path, applications: &Path) -> Vec<PathBuf> {
        let mut drives = Vec::new();
        let app_folders = [
            Some(applications.to_path_buf()),
            real_folder(home, "Applications"),
        ];
        for folder in app_folders.iter().flatten() {
            for app in subfolders(folder).filter(|app| app.extension() == Some(OsStr::new("app"))) {
                drives.extend(real_folder(&app, "Contents/SharedSupport/prefix/drive_c"));
                // Wineskin wrappers made before version 2.9.
                drives.extend(real_folder(&app, "Contents/Resources/drive_c"));
            }
        }
        if let Some(bottles) = real_folder(home, "Library/Application Support/CrossOver/Bottles") {
            drives
                .extend(subfolders(&bottles).filter_map(|bottle| real_folder(&bottle, "drive_c")));
        }
        drives.extend(real_folder(home, ".wine/drive_c"));

        let mut found = Vec::new();
        for drive in &drives {
            game_folders_below(drive, &mut found);
        }
        found.sort();
        found
    }

    /// `below` inside `folder`, when each folder on the way is a real folder.
    /// A link, such as a ~/.wine moved to an external drive or into Documents,
    /// could lead to a place macOS asks the player about.
    fn real_folder(folder: &Path, below: &str) -> Option<PathBuf> {
        let mut path = folder.to_path_buf();
        for name in below.split('/') {
            path.push(name);
            if !std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_dir()) {
                return None;
            }
        }
        Some(path)
    }

    /// Add the folders below `drive` that hold both masters to `found`.
    fn game_folders_below(drive: &Path, found: &mut Vec<PathBuf>) {
        let mut pending = vec![(drive.to_path_buf(), 0)];
        let mut read = 0;
        while let Some((folder, depth)) = pending.pop() {
            if read == SEARCH_LIMIT {
                log::warn!(
                    "Stopped looking for the game after {read} folders in {}",
                    drive.display()
                );
                return;
            }
            read += 1;
            if holds_master_archives(&folder) {
                found.push(folder.clone());
            }
            if depth < SEARCH_DEPTH {
                let below = subfolders(&folder).filter(|sub| {
                    let name = sub.file_name().unwrap_or_default().to_string_lossy();
                    !name.starts_with('.')
                        && (depth > 0
                            || !WINDOWS_FOLDERS.iter().any(|w| name.eq_ignore_ascii_case(w)))
                });
                pending.extend(below.map(|sub| (sub, depth + 1)));
            }
        }
    }

    /// The real folders in `folder`; links are not followed, so the search
    /// stays inside the prefix.
    fn subfolders(folder: &Path) -> impl Iterator<Item = PathBuf> {
        std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.path())
    }

    /// The dialogs' words, in Chinese when that is the player's first
    /// language and in English otherwise.
    struct Text {
        found: &'static str,
        found_at: &'static str,
        use_folder: &'static str,
        choose_another: &'static str,
        quit: &'static str,
        choose: &'static str,
        retry: &'static str,
    }

    const ENGLISH: Text = Text {
        found: "Use this Yuri's Revenge folder?",
        found_at: "VERA20k found the game here:",
        use_folder: "Use This Folder",
        choose_another: "Choose Another…",
        quit: "Quit",
        choose: "Choose your Yuri's Revenge folder, the one holding ra2md.mix and ra2.mix.",
        retry: "That folder has no ra2md.mix and ra2.mix. Choose your Yuri's Revenge folder.",
    };

    const CHINESE: Text = Text {
        found: "使用这个《尤里的复仇》游戏文件夹吗？",
        found_at: "VERA20k 在这里找到了游戏：",
        use_folder: "使用此文件夹",
        choose_another: "选择其他文件夹…",
        quit: "退出",
        choose: "请选择《尤里的复仇》游戏文件夹（包含 ra2md.mix 和 ra2.mix 的文件夹）。",
        retry: "该文件夹中没有 ra2md.mix 和 ra2.mix。请重新选择《尤里的复仇》游戏文件夹。",
    };

    /// The system's alert and folder chooser, run modally by this process.
    struct SystemDialogs {
        mtm: MainThreadMarker,
        text: &'static Text,
    }

    impl SystemDialogs {
        fn new() -> Result<Self> {
            let mtm = MainThreadMarker::new()
                .context("The game folder questions need the main thread")?;
            let chinese = objc2_foundation::NSLocale::preferredLanguages()
                .firstObject()
                .is_some_and(|language| language.to_string().starts_with("zh"));
            Ok(Self {
                mtm,
                text: if chinese { &CHINESE } else { &ENGLISH },
            })
        }
    }

    impl Player for SystemDialogs {
        fn offer(&mut self, folder: &Path) -> Result<Offer> {
            use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSAlertSecondButtonReturn};
            use objc2_foundation::NSString;

            let alert = NSAlert::new(self.mtm);
            alert.setMessageText(&NSString::from_str(self.text.found));
            alert.setInformativeText(&NSString::from_str(&format!(
                "{}\n{}",
                self.text.found_at,
                folder.display()
            )));
            for title in [
                self.text.use_folder,
                self.text.choose_another,
                self.text.quit,
            ] {
                alert.addButtonWithTitle(&NSString::from_str(title));
            }
            let response = alert.runModal();
            Ok(if response == NSAlertFirstButtonReturn {
                Offer::Use
            } else if response == NSAlertSecondButtonReturn {
                Offer::ChooseAnother
            } else {
                Offer::Quit
            })
        }

        fn choose(&mut self, start: Option<&Path>, retry: bool) -> Result<Option<PathBuf>> {
            use objc2_app_kit::{NSModalResponseOK, NSOpenPanel};
            use objc2_foundation::{NSString, NSURL};

            let panel = NSOpenPanel::openPanel(self.mtm);
            panel.setCanChooseDirectories(true);
            panel.setCanChooseFiles(false);
            panel.setAllowsMultipleSelection(false);
            // Porting Kit and Wineskin keep the game inside their wrapper app.
            panel.setTreatsFilePackagesAsDirectories(true);
            panel.setPrompt(Some(&NSString::from_str(self.text.use_folder)));
            let message = if retry {
                self.text.retry
            } else {
                self.text.choose
            };
            panel.setMessage(Some(&NSString::from_str(message)));
            if let Some(start) = start.and_then(NSURL::from_directory_path) {
                panel.setDirectoryURL(Some(&start));
            }
            if panel.runModal() != NSModalResponseOK {
                return Ok(None);
            }
            panel
                .URL()
                .and_then(|url| url.to_file_path())
                .context("The folder chooser returned no folder")
                .map(Some)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_executable_inside_an_app_bundle_counts() {
        assert!(in_app_bundle(Path::new(
            "/Applications/VERA20k.app/Contents/MacOS/VERA20k"
        )));
        assert!(!in_app_bundle(Path::new(
            "/Users/me/vera20k/target/release/vera20k"
        )));
        assert!(!in_app_bundle(Path::new("/Contents/MacOS/vera20k")));
    }

    #[cfg(target_os = "macos")]
    mod first_launch {
        use std::path::{Path, PathBuf};

        use anyhow::Result;

        use super::super::first_launch::{
            Offer, Player, ensure_game_folder_with, find_game_folders,
        };
        use crate::util::config::GameConfig;

        struct Fixture {
            root: PathBuf,
            data: PathBuf,
            executable: PathBuf,
            game: PathBuf,
        }

        impl Fixture {
            fn new() -> Self {
                static NEXT: std::sync::atomic::AtomicUsize =
                    std::sync::atomic::AtomicUsize::new(0);
                let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let root = std::env::temp_dir()
                    .join(format!("vera20k-app-bundle-{}-{id}", std::process::id()));
                let data = root.join("data");
                std::fs::create_dir_all(&data).unwrap();
                let game = root.join("Yuri's Revenge 尤里的复仇");
                write_masters(&game, ["RA2MD.MIX", "ra2.mix"]);
                let executable = root.join("VERA20k.app/Contents/MacOS/VERA20k");
                Self {
                    root,
                    data,
                    executable,
                    game,
                }
            }

            fn ensure(&self, found: &[&Path], player: &mut Script) -> bool {
                let found = found.iter().map(|path| path.to_path_buf()).collect();
                ensure_game_folder_with(&self.data, &self.executable, || found, player).unwrap()
            }

            fn load(&self) -> GameConfig {
                GameConfig::load_from(&self.data, &self.executable).unwrap()
            }
        }

        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.root);
            }
        }

        fn write_masters<const N: usize>(folder: &Path, names: [&str; N]) {
            std::fs::create_dir_all(folder).unwrap();
            for name in names {
                std::fs::write(folder.join(name), b"").unwrap();
            }
        }

        /// A player who answers from a script and records each question.
        #[derive(Default)]
        struct Script {
            offers: Vec<Offer>,
            choices: Vec<Option<PathBuf>>,
            asked: Vec<String>,
        }

        impl Player for Script {
            fn offer(&mut self, folder: &Path) -> Result<Offer> {
                self.asked.push(format!("offer {}", folder.display()));
                Ok(self.offers.remove(0))
            }

            fn choose(&mut self, start: Option<&Path>, retry: bool) -> Result<Option<PathBuf>> {
                self.asked
                    .push(format!("choose from {start:?}, retry {retry}"));
                Ok(self.choices.remove(0))
            }
        }

        #[test]
        fn a_found_game_folder_takes_one_answer() {
            let fixture = Fixture::new();
            let mut player = Script {
                offers: vec![Offer::Use],
                ..Script::default()
            };
            assert!(fixture.ensure(&[fixture.game.as_path()], &mut player));
            assert_eq!(player.asked, [format!("offer {}", fixture.game.display())]);

            let config = fixture.load();
            assert_eq!(config.paths.ra2_dir, fixture.game);
            assert_eq!(config.paths.settings_dir(), fixture.data);
            let written = std::fs::read_to_string(fixture.data.join("config.toml")).unwrap();
            assert!(written.contains("[graphics]"), "{written}");
            // The next launch finds the folder in config.toml and asks nothing.
            let mut silent = Script::default();
            assert!(fixture.ensure(&[], &mut silent));
            assert!(silent.asked.is_empty());
        }

        #[test]
        fn another_folder_is_chosen_beside_the_found_one_until_it_holds_the_game() {
            let fixture = Fixture::new();
            let found = fixture.root.join("prefix/drive_c/YuriRevenge");
            write_masters(&found, ["ra2md.mix", "ra2.mix"]);
            let wrong = fixture.root.join("wrong");
            std::fs::create_dir(&wrong).unwrap();
            let mut player = Script {
                offers: vec![Offer::ChooseAnother],
                choices: vec![Some(wrong), Some(fixture.game.clone())],
                ..Script::default()
            };
            assert!(fixture.ensure(&[found.as_path()], &mut player));
            let start = found.parent().unwrap();
            assert_eq!(
                player.asked,
                [
                    format!("offer {}", found.display()),
                    format!("choose from {:?}, retry false", Some(start)),
                    format!("choose from {:?}, retry true", Some(start)),
                ]
            );
            assert_eq!(fixture.load().paths.ra2_dir, fixture.game);
        }

        #[test]
        fn quitting_or_cancelling_writes_nothing() {
            let fixture = Fixture::new();
            let config = fixture.data.join("config.toml");
            let mut quits = Script {
                offers: vec![Offer::Quit],
                ..Script::default()
            };
            assert!(!fixture.ensure(&[fixture.game.as_path()], &mut quits));
            let mut cancels = Script {
                choices: vec![None],
                ..Script::default()
            };
            assert!(!fixture.ensure(&[], &mut cancels));
            assert_eq!(cancels.asked, ["choose from None, retry false"]);
            assert!(!config.exists());
        }

        #[test]
        fn a_moved_game_folder_is_set_up_again_keeping_other_settings() {
            let fixture = Fixture::new();
            std::fs::write(
                fixture.data.join("config.toml"),
                "[paths]\nra2_dir = 'moved away'\nsettings_dir = '.'\n[profile]\nname = '指挥官'\n",
            )
            .unwrap();
            let mut player = Script {
                offers: vec![Offer::Use],
                ..Script::default()
            };
            assert!(fixture.ensure(&[fixture.game.as_path()], &mut player));
            let config = fixture.load();
            assert_eq!(config.paths.ra2_dir, fixture.game);
            assert_eq!(config.paths.settings_dir(), fixture.data);
            assert_eq!(config.profile.player_name(), Some("指挥官"));
        }

        #[test]
        fn a_broken_config_is_left_to_the_error_screen() {
            let fixture = Fixture::new();
            let config = fixture.data.join("config.toml");
            std::fs::write(&config, "[broken").unwrap();
            let mut player = Script::default();
            assert!(fixture.ensure(&[fixture.game.as_path()], &mut player));
            assert!(player.asked.is_empty());
            assert_eq!(std::fs::read_to_string(&config).unwrap(), "[broken");
        }

        #[test]
        fn the_search_finds_the_game_in_wrapper_apps_bottles_and_wine() {
            let fixture = Fixture::new();
            let applications = fixture.root.join("Applications");
            let home = fixture.root.join("home");
            let porting_kit =
                applications.join("C C Red Alert 2.app/Contents/SharedSupport/prefix/drive_c");
            let old_wineskin = home.join("Applications/RA2.app/Contents/Resources/drive_c");
            let steam = home.join(
                "Library/Application Support/CrossOver/Bottles/Steam/drive_c/Program Files (x86)/\
                 Steam/steamapps/common/Command & Conquer Red Alert II",
            );
            let wine = home.join(".wine/drive_c");
            write_masters(&porting_kit.join("YuriRevenge"), ["RA2MD.MIX", "RA2.MIX"]);
            write_masters(&old_wineskin.join("Westwood/RA2"), ["ra2md.mix", "ra2.mix"]);
            write_masters(&steam, ["ra2md.mix", "ra2.mix"]);
            // Not the game: one master only, Windows' own folders, hidden
            // folders, and anything deeper than six folders.
            write_masters(&wine.join("Games/Half"), ["ra2md.mix"]);
            write_masters(
                &porting_kit.join("windows/system32"),
                ["ra2md.mix", "ra2.mix"],
            );
            write_masters(&wine.join(".cache/RA2"), ["ra2md.mix", "ra2.mix"]);
            write_masters(&wine.join("a/b/c/d/e/f/g"), ["ra2md.mix", "ra2.mix"]);

            assert_eq!(
                find_game_folders(&home, &applications),
                [
                    porting_kit.join("YuriRevenge"),
                    old_wineskin.join("Westwood/RA2"),
                    steam,
                ]
            );
            assert!(
                find_game_folders(&fixture.root.join("nobody"), &fixture.root.join("none"))
                    .is_empty()
            );
        }

        #[test]
        fn the_search_follows_no_link_out_of_a_prefix() {
            use std::os::unix::fs::symlink;

            let fixture = Fixture::new();
            let applications = fixture.root.join("Applications");
            let home = fixture.root.join("home");
            // A real prefix, then links to one elsewhere: a moved ~/.wine, a
            // wrapper's drive_c and the CrossOver bottles folder.
            let wrapper = applications.join("RA2.app/Contents/SharedSupport/prefix");
            write_masters(&wrapper.join("drive_c/RA2"), ["ra2md.mix", "ra2.mix"]);
            let elsewhere = fixture.root.join("Documents/prefix");
            write_masters(&elsewhere.join("drive_c/RA2"), ["ra2md.mix", "ra2.mix"]);
            let linked = applications.join("Linked.app/Contents/SharedSupport/prefix");
            std::fs::create_dir_all(&linked).unwrap();
            symlink(elsewhere.join("drive_c"), linked.join("drive_c")).unwrap();
            std::fs::create_dir_all(home.join("Library/Application Support/CrossOver")).unwrap();
            symlink(
                fixture.root.join("Documents"),
                home.join("Library/Application Support/CrossOver/Bottles"),
            )
            .unwrap();
            symlink(&elsewhere, home.join(".wine")).unwrap();

            assert_eq!(
                find_game_folders(&home, &applications),
                [wrapper.join("drive_c/RA2")]
            );
        }
    }
}
