//! VERA's writable player profile. Retail settings are import sources only.

use std::io;
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::rules::ini_parser::IniFile;
use crate::util::case_insensitive_path::find;
use crate::util::config::PathsConfig;
use crate::util::ini_writer::replace_ini_section;

pub(crate) const FILENAME: &str = "VERA.INI";

pub(crate) fn path(directory: &Path) -> PathBuf {
    find(directory, FILENAME).unwrap_or_else(|| directory.join(FILENAME))
}

/// Import once, before any of the profile's owners load their sections. Keep
/// the old files byte-for-byte, including profiles outside the retail folder.
pub(crate) fn import_legacy(paths: &PathsConfig) -> io::Result<()> {
    let destination = path(paths.settings_dir());
    if crate::util::file_system::try_exists(&destination)? {
        return Ok(());
    }
    let sources = [paths.legacy_settings_dir(), paths.ra2_dir.as_path()];
    let mut profile = read_legacy(&sources, "RA2MD.INI")?.unwrap_or_default();
    if let Some(keyboard) = read_legacy(&sources, "KeyboardMD.ini")? {
        let ini = IniFile::from_bytes(&keyboard)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        if let Some(hotkeys) = ini.section("Hotkey") {
            profile = replace_ini_section(
                &profile,
                "Hotkey",
                &hotkeys.raw_entries().collect::<Vec<_>>(),
            );
        }
    }
    crate::util::file_system::create_dir_all(paths.settings_dir())?;
    // These retain their own schemas and owners, but move out of the retail
    // folder with the options they accompany.
    for name in ["wolinfoMD.ini", "coopsave.ini"] {
        if find(paths.settings_dir(), name).is_none()
            && let Some(bytes) = read_legacy(&sources, name)?
        {
            create_new(&paths.settings_dir().join(name), &bytes)?;
        }
    }
    // Even an empty profile records that import has happened. Deleting/resetting
    // a section later must not bring an old retail preference back.
    create_new(&destination, &profile)
}

fn read_legacy(directories: &[&Path], name: &str) -> io::Result<Option<Vec<u8>>> {
    for directory in directories {
        if let Some(path) = find(directory, name) {
            return crate::util::file_system::read(path).map(Some);
        }
    }
    Ok(None)
}

#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
pub(super) fn create_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => match file.write_all(bytes) {
            Ok(()) => Ok(()),
            Err(error) => {
                drop(file);
                let _ = crate::util::file_system::remove_file(path);
                Err(error)
            }
        },
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error),
    }
}

/// Read current disk bytes for every update, so independent section owners
/// cannot erase each other's newer settings.
pub(crate) fn update(directory: &Path, transform: impl FnOnce(&[u8]) -> Vec<u8>) -> io::Result<()> {
    crate::util::file_system::create_dir_all(directory)?;
    update_with(
        directory,
        |path| crate::util::file_system::read(path),
        |path, bytes| crate::util::file_system::write(path, bytes),
        transform,
    )
}

pub(super) fn update_with(
    directory: &Path,
    read: impl FnOnce(&Path) -> io::Result<Vec<u8>>,
    write: impl FnOnce(&Path, &[u8]) -> io::Result<()>,
    transform: impl FnOnce(&[u8]) -> Vec<u8>,
) -> io::Result<()> {
    let path = path(directory);
    let input = match read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    write(&path, &transform(&input))
}

#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
pub(super) fn create_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    match crate::util::file_system::write_new(path, bytes) {
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::frontend::startup_options::RetailStartupOptions;
    use crate::app::input::hotkeys::HotkeyCommand;
    use crate::app::persistence::{keyboard, options_profile::RetailOptionsLoad};
    use crate::util::config::GameConfig;

    #[test]
    fn imports_once_without_changing_retail_files_and_preserves_all_sections() {
        let root = std::env::temp_dir().join(format!("vera-profile-import-{}", std::process::id()));
        let retail = root.join("retail");
        let settings = root.join("profile");
        std::fs::create_dir_all(&retail).unwrap();
        let original = b"; retail\r\n[Audio]\r\nSoundVolume=0.25\r\n[Skirmish]\r\nName=\xE9\r\n";
        let hotkeys = b"[Hotkey]\nStopObject=88\n";
        std::fs::write(retail.join("ra2md.ini"), original).unwrap();
        std::fs::write(retail.join("KEYBOARDMD.INI"), hotkeys).unwrap();
        std::fs::create_dir(&settings).unwrap();
        let previous = b"[Audio]\nSoundVolume=0.6\n[Skirmish]\nName=\xE9\n";
        std::fs::write(settings.join("RA2MD.INI"), previous).unwrap();
        std::fs::write(retail.join("coopsave.ini"), b"[Progress]\nCurrentMap=2\n").unwrap();
        std::fs::write(
            root.join("config.toml"),
            "[paths]\nra2_dir='retail'\nsettings_dir='profile'\n",
        )
        .unwrap();
        let config = GameConfig::load_from(&root, &root.join("vera20k")).unwrap();
        import_legacy(&config.paths).unwrap();
        let profile = RetailOptionsLoad::from_settings(&settings, &RetailStartupOptions::default())
            .retained_profile;
        assert_eq!(profile.sound_volume, 0.6);
        assert_eq!(
            keyboard::reload(&settings, None).first_key(HotkeyCommand::StopObject),
            Some(88)
        );
        assert!(
            std::fs::read(path(&settings))
                .unwrap()
                .starts_with(previous)
        );
        assert_eq!(
            std::fs::read(settings.join("coopsave.ini")).unwrap(),
            b"[Progress]\nCurrentMap=2\n"
        );
        keyboard::reset(&settings, Some(b"[Hotkey]\nStopObject=83\n"));
        profile.commit_settings(&settings).unwrap();
        import_legacy(&config.paths).unwrap();
        assert_eq!(
            keyboard::reload(&settings, None).first_key(HotkeyCommand::StopObject),
            Some(83)
        );
        assert_eq!(std::fs::read(retail.join("ra2md.ini")).unwrap(), original);
        assert_eq!(
            std::fs::read(retail.join("KEYBOARDMD.INI")).unwrap(),
            hotkeys
        );
        assert_eq!(std::fs::read(settings.join("RA2MD.INI")).unwrap(), previous);
        assert!(
            std::fs::read(path(&settings))
                .unwrap()
                .windows(6)
                .any(|bytes| bytes == b"Name=\xE9")
        );
        assert!(!retail.join(FILENAME).exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
