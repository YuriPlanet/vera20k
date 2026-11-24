//! Install the release's loose Arctic Circle map into the player's Maps folder.
use crate::util::{case_insensitive_path, file_system as fs};
use std::io;
use std::path::Path;

pub const ARCTIC_MAP: &str = "Arctic_Circle_16_Ice_Passages.map";
const RECEIPT: &str = ".Arctic_Circle_16_Ice_Passages.installed";

/// Record the first installation even if the player already had a copy.
/// Subsequent launches respect edits and deletions.
pub fn install_arctic_map(
    maps_dir: &Path,
    read_package: impl FnOnce() -> io::Result<Vec<u8>>,
) -> io::Result<()> {
    let receipt = maps_dir.join(RECEIPT);
    if fs::try_exists(&receipt)? {
        return Ok(());
    }
    if case_insensitive_path::find(maps_dir, ARCTIC_MAP).is_none() {
        let bytes = read_package()?;
        fs::create_dir_all(maps_dir)?;
        write_if_absent(&maps_dir.join(ARCTIC_MAP), &bytes)?;
    }
    write_if_absent(&receipt, b"")
}

fn write_if_absent(path: &Path, bytes: &[u8]) -> io::Result<()> {
    match fs::write_new(path, bytes) {
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        result => result,
    }
}

/// Windows installs directly into Maps by default. App bundles and custom
/// profiles need a writable copy. Source checkouts also ship the loose file.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn install_desktop_map(maps_dir: &Path) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let packaged = exe
        .parent()
        .and_then(|dir| crate::util::platform_dir::packaged_resource_dir(dir, "Maps"));
    let source_dir = packaged.or_else(|| {
        let cwd = std::env::current_dir().ok()?;
        cwd.join("src/map/mod.rs")
            .is_file()
            .then(|| cwd.join("maps"))
    });
    let Some(source_dir) = source_dir else {
        return Ok(());
    };
    let source = source_dir.join(ARCTIC_MAP);
    if !source.is_file() {
        return Ok(());
    }
    install_arctic_map(maps_dir, || fs::read(source))
}

#[cfg(all(test, tests_map))]
mod tests {
    use super::*;

    #[test]
    fn first_launch_installs_a_complete_map_and_respects_later_removal() {
        let root = tempfile::tempdir().unwrap();
        let maps = root.path().join("Maps");
        assert!(install_arctic_map(&maps, || Err(io::ErrorKind::NotFound.into())).is_err());
        install_arctic_map(&maps, || Ok(b"packaged map".to_vec())).unwrap();
        let file = maps.join(ARCTIC_MAP);
        assert_eq!(fs::read(&file).unwrap(), b"packaged map");
        fs::remove_file(&file).unwrap();
        install_arctic_map(&maps, || panic!("map was already installed")).unwrap();
        assert!(!file.exists());
    }

    #[test]
    fn existing_copy_keeps_its_contents_and_filename() {
        let maps = tempfile::tempdir().unwrap();
        let file = maps.path().join(ARCTIC_MAP.to_ascii_lowercase());
        fs::write(&file, b"player edit").unwrap();
        install_arctic_map(maps.path(), || panic!("player already has a copy")).unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"player edit");
        assert_eq!(fs::read_dir(maps.path()).unwrap().count(), 2);
    }
}
