//! Starting VERA20k from its macOS app bundle.
//!
//! Finder starts an app in `/`, and the bundle can be read-only: a mounted disk
//! image, or a downloaded copy that macOS runs from a randomized location (App
//! Translocation). A launch from `VERA20k.app` therefore works in
//! `~/Library/Application Support/VERA20k`, the way the Windows shortcuts start
//! the game in its install folder: config.toml, RA2MD.INI, logs, saves and
//! replays all live there. Until config.toml names a Yuri's Revenge folder,
//! each launch asks for one with the system folder chooser and writes it there.
//!
//! The chooser runs in the event loop's first `resumed`, before any window
//! exists: macOS brings forward only the app it launched, once that app is
//! running. A chooser shown earlier by a helper process (`osascript`) stayed
//! hidden behind the app the player was using.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::assets::asset_manager::holds_master_archives;
use crate::util::config::{GameConfig, set_paths_key};

/// The data folder, below the home folder.
const DATA_FOLDER: &str = "Library/Application Support/VERA20k";
/// A first launch starts config.toml from this, so the file documents every setting.
const CONFIG_TEMPLATE: &str = include_str!("../../config.toml.example");

const CHOOSE_PROMPT: &str = "Choose your Yuri's Revenge folder, the one holding ra2md.mix and ra2.mix.\n\
     请选择《尤里的复仇》游戏文件夹（包含 ra2md.mix 和 ra2.mix 的文件夹）。";
const RETRY_PROMPT: &str = "That folder has no ra2md.mix and ra2.mix. Choose your Yuri's Revenge folder.\n\
     该文件夹中没有 ra2md.mix 和 ra2.mix。请重新选择《尤里的复仇》游戏文件夹。";

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

/// Make sure config.toml in the data folder names a Yuri's Revenge folder,
/// asking for one until it does. Returns false when the player cancels.
pub fn ensure_game_folder() -> bool {
    let asked = (|| -> Result<bool> {
        let data_folder = std::env::current_dir()?;
        let executable = std::env::current_exe()?;
        ensure_game_folder_with(&data_folder, &executable, choose_folder)
    })();
    asked.unwrap_or_else(|error| {
        // The startup error screen still says which game files are missing.
        log::warn!("Could not ask for the game folder: {error:#}");
        true
    })
}

fn ensure_game_folder_with(
    data_folder: &Path,
    executable: &Path,
    mut choose: impl FnMut(&str) -> Result<Option<PathBuf>>,
) -> Result<bool> {
    // A config.toml that does not load is left to the startup error screen,
    // which names its problem.
    match GameConfig::load_from(data_folder, executable) {
        Ok(config) if !holds_master_archives(&config.paths.ra2_dir) => {}
        _ => return Ok(true),
    }
    let mut prompt = CHOOSE_PROMPT;
    let folder = loop {
        let Some(folder) = choose(prompt)? else {
            return Ok(false);
        };
        if holds_master_archives(&folder) {
            break folder;
        }
        log::info!("No ra2md.mix and ra2.mix in {}", folder.display());
        prompt = RETRY_PROMPT;
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
    std::fs::write(&path, config).with_context(|| format!("Failed to write {}", path.display()))?;
    log::info!("Game folder {ra2_dir} written to {}", path.display());
    Ok(true)
}

/// The system folder chooser; None means the player cancelled. It needs the
/// launched app: call it on the main thread from the event loop.
#[cfg(target_os = "macos")]
fn choose_folder(prompt: &str) -> Result<Option<PathBuf>> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalResponseOK, NSOpenPanel};
    use objc2_foundation::NSString;

    let mtm = MainThreadMarker::new().context("The folder chooser needs the main thread")?;
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseDirectories(true);
    panel.setCanChooseFiles(false);
    panel.setAllowsMultipleSelection(false);
    panel.setMessage(Some(&NSString::from_str(prompt)));
    if panel.runModal() != NSModalResponseOK {
        return Ok(None);
    }
    panel
        .URL()
        .and_then(|url| url.to_file_path())
        .context("The folder chooser returned no folder")
        .map(Some)
}

#[cfg(not(target_os = "macos"))]
fn choose_folder(_prompt: &str) -> Result<Option<PathBuf>> {
    anyhow::bail!("Only the macOS app asks for the game folder")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
        data: PathBuf,
        executable: PathBuf,
        game: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let root = std::env::temp_dir()
                .join(format!("vera20k-app-bundle-{}-{id}", std::process::id()));
            let data = root.join("data");
            let game = root.join("Yuri's Revenge 尤里的复仇");
            std::fs::create_dir_all(&data).unwrap();
            std::fs::create_dir_all(&game).unwrap();
            std::fs::write(game.join("RA2MD.MIX"), b"").unwrap();
            std::fs::write(game.join("ra2.mix"), b"").unwrap();
            let executable = root.join("VERA20k.app/Contents/MacOS/vera20k");
            Self {
                root,
                data,
                executable,
                game,
            }
        }

        fn ensure(&self, choose: impl FnMut(&str) -> Result<Option<PathBuf>>) -> bool {
            ensure_game_folder_with(&self.data, &self.executable, choose).unwrap()
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

    #[test]
    fn a_first_launch_asks_until_the_folder_holds_the_game() {
        let fixture = Fixture::new();
        let wrong = fixture.root.join("wrong");
        std::fs::create_dir(&wrong).unwrap();
        let mut answers = [Some(wrong), Some(fixture.game.clone())].into_iter();
        let mut prompts = Vec::new();
        assert!(fixture.ensure(|prompt| {
            prompts.push(prompt.to_owned());
            Ok(answers.next().unwrap())
        }));
        assert_eq!(prompts, [CHOOSE_PROMPT, RETRY_PROMPT]);

        let config = fixture.load();
        assert_eq!(config.paths.ra2_dir, fixture.game);
        assert_eq!(config.paths.settings_dir(), fixture.data);
        let written = std::fs::read_to_string(fixture.data.join("config.toml")).unwrap();
        assert!(written.contains("[graphics]"), "{written}");
        // The next launch finds the folder and asks nothing.
        assert!(fixture.ensure(|_| panic!("asked again")));
    }

    #[test]
    fn a_moved_game_folder_is_asked_for_again_keeping_other_settings() {
        let fixture = Fixture::new();
        std::fs::write(
            fixture.data.join("config.toml"),
            "[paths]\nra2_dir = 'moved away'\nsettings_dir = '.'\n[profile]\nname = '指挥官'\n",
        )
        .unwrap();
        assert!(fixture.ensure(|_| Ok(Some(fixture.game.clone()))));
        let config = fixture.load();
        assert_eq!(config.paths.ra2_dir, fixture.game);
        assert_eq!(config.paths.settings_dir(), fixture.data);
        assert_eq!(config.profile.player_name(), Some("指挥官"));
    }

    #[test]
    fn cancelling_or_a_broken_config_writes_nothing() {
        let fixture = Fixture::new();
        let config = fixture.data.join("config.toml");
        assert!(!fixture.ensure(|_| Ok(None)));
        assert!(!config.exists());

        std::fs::write(&config, "[broken").unwrap();
        assert!(fixture.ensure(|_| panic!("asked")));
        assert_eq!(std::fs::read_to_string(&config).unwrap(), "[broken");
    }

    #[test]
    fn only_an_executable_inside_an_app_bundle_counts() {
        assert!(in_app_bundle(Path::new(
            "/Applications/VERA20k.app/Contents/MacOS/vera20k"
        )));
        assert!(!in_app_bundle(Path::new(
            "/Users/me/vera20k/target/release/vera20k"
        )));
        assert!(!in_app_bundle(Path::new("/Contents/MacOS/vera20k")));
    }
}
