//! Bundle assets and sandbox-writable state for the native iOS application.
//!
//! Called before logging or worker threads start. The ordinary config loader,
//! saved-game repository and logger then keep their existing responsibilities.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

/// Locate the installed bundle and set the process's writable working directory.
#[cfg(target_os = "ios")]
pub fn prepare() -> Result<()> {
    let executable = std::env::current_exe().context("locate iOS executable")?;
    let bundle = executable.parent().context("locate iOS app bundle")?;
    let home = std::env::var_os("HOME").context("iOS sandbox HOME is unavailable")?;
    let runtime = prepare_directories(bundle, Path::new(&home))?;
    std::env::set_current_dir(&runtime).context("select writable iOS runtime directory")?;
    eprintln!("VERA20k iOS runtime: {}", runtime.display());
    Ok(())
}

fn prepare_directories(bundle: &Path, home: &Path) -> Result<PathBuf> {
    let assets = bundle.join("retail-assets");
    ensure!(
        assets.is_dir(),
        "bundled retail assets missing: {}",
        assets.display()
    );
    let runtime = home.join("Library/Application Support/VERA20k");
    let profile = runtime.join("profile");
    fs::create_dir_all(&profile).context("create writable iOS profile directory")?;
    fs::create_dir_all(runtime.join("saves")).context("create writable iOS saves directory")?;
    fs::create_dir_all(runtime.join("logs")).context("create writable iOS logs directory")?;

    let saved_profile = profile.join("RA2MD.INI");
    if !saved_profile.exists() {
        let seed = assets.join("RA2MD.INI");
        if seed.is_file() {
            fs::copy(seed, &saved_profile).context("seed first-run iOS game settings")?;
        }
    }
    // Bundle UUIDs change on reinstall. Refresh only this generated path config;
    // retail settings and saves in the container remain owned by their readers.
    let config = format!(
        "[paths]\nra2_dir = {:?}\nsettings_dir = {:?}\n\n\
         [graphics]\nextra_animations = true\n\n\
         [gameplay]\ninput_delay_ticks = 2\npause_on_focus_loss = true\n\n\
         [profile]\nname = \"BXY\"\n",
        assets.to_string_lossy(),
        profile.to_string_lossy(),
    );
    fs::write(runtime.join("config.toml"), config).context("write iOS runtime paths")?;
    Ok(runtime)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reinstall_updates_bundle_paths_and_preserves_settings_and_saves() {
        let scratch = std::env::temp_dir().join(format!(
            "vera20k-ios-bootstrap-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let old_bundle = scratch.join("old/VERA20k.app");
        let new_bundle = scratch.join("new/VERA20k.app");
        for bundle in [&old_bundle, &new_bundle] {
            let assets = bundle.join("retail-assets");
            fs::create_dir_all(&assets).unwrap();
            fs::write(assets.join("RA2MD.INI"), "[Skirmish]\nCredits=10000\n").unwrap();
        }
        let home = scratch.join("sandbox");
        let runtime = prepare_directories(&old_bundle, &home).unwrap();
        fs::write(
            runtime.join("profile/RA2MD.INI"),
            "[Skirmish]\nCredits=5000\n",
        )
        .unwrap();
        fs::write(runtime.join("saves/player.sav"), b"player data").unwrap();
        assert_eq!(prepare_directories(&new_bundle, &home).unwrap(), runtime);
        let config = fs::read_to_string(runtime.join("config.toml")).unwrap();
        assert!(config.contains(new_bundle.to_str().unwrap()));
        assert!(!config.contains(old_bundle.to_str().unwrap()));
        assert!(config.contains("pause_on_focus_loss = true"));
        assert_eq!(
            fs::read_to_string(runtime.join("profile/RA2MD.INI")).unwrap(),
            "[Skirmish]\nCredits=5000\n"
        );
        assert_eq!(
            fs::read(runtime.join("saves/player.sav")).unwrap(),
            b"player data"
        );
        assert!(!new_bundle.join("config.toml").exists());
        fs::remove_dir_all(scratch).unwrap();
    }
}
