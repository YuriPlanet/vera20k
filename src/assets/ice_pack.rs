//! The standard snow extension. Its two installed files must match the build
//! before any of their tile IDs, land types or movement data enter a match.
use std::io;
use std::path::{Path, PathBuf};

pub const DIRECTORY: &str = "Terrain";
type FileIdentity = (&'static str, usize, [u8; 32]);
const FILES: [FileIdentity; 2] = include!(concat!(env!("OUT_DIR"), "/ice_file_identities.rs"));
pub const FILE_NAMES: [&str; 2] = [FILES[0].0, FILES[1].0];

// A browser imports the pack at /Terrain, and a WASI host preopens it there.
#[cfg(any(
    target_os = "wasi",
    all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")
))]
fn terrain_directory() -> io::Result<PathBuf> {
    Ok(Path::new("/").join(DIRECTORY))
}

#[cfg(target_os = "android")]
fn terrain_directory() -> io::Result<PathBuf> {
    crate::util::platform_dir::get()
        .map(|dir| dir.join(DIRECTORY))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Android data folder unavailable"))
}

#[cfg(not(any(
    target_os = "android",
    target_os = "wasi",
    all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")
)))]
fn terrain_directory() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let program = exe
        .parent()
        .ok_or_else(|| io::Error::other("game executable has no directory"))?;
    if let Some(packaged) = crate::util::platform_dir::packaged_resource_dir(program, DIRECTORY) {
        return Ok(packaged);
    }
    // When Cargo runs the uninstalled game or tools, use the pack from the
    // checkout that compiled them. Labeled games carry their own Terrain.
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    if program
        .ancestors()
        .any(|dir| dir.join(".rustc_info.json").is_file())
        || std::env::current_dir()
            .is_ok_and(|dir| dir == manifest || Some(dir.as_path()) == manifest.parent())
    {
        return Ok(manifest.join("..").join(DIRECTORY));
    }
    Ok(program.join(DIRECTORY))
}

fn verify(bytes: &[u8], (name, size, digest): FileIdentity) -> io::Result<()> {
    if bytes.len() != size || crate::util::sha256::sha256_bytes(bytes) != digest {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{DIRECTORY}/{name} does not match this build"),
        ));
    }
    Ok(())
}

#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
fn read_bounded(path: &Path, size: usize) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::with_capacity(size);
    std::fs::File::open(path)?
        .take(size as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn verified_file(dir: &Path, file: FileIdentity) -> io::Result<Vec<u8>> {
    let path = dir.join(file.0);
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    {
        let bytes = read_bounded(&path, file.1)?;
        verify(&bytes, file)?;
        Ok(bytes)
    }
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    {
        let bytes = crate::util::file_system::read_shared(&path)?;
        verify(&bytes, file)?;
        Ok(bytes.to_vec())
    }
}

/// An ice map pins the verified pair for its theater load.
pub fn load() -> io::Result<(Vec<u8>, Vec<u8>)> {
    let dir = terrain_directory()?;
    let read = |file: FileIdentity| {
        verified_file(&dir, file)
            .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", dir.join(file.0).display())))
    };
    Ok((read(FILES[0])?, read(FILES[1])?))
}

/// Publish the two standard resources from a read-only package into an
/// accessible Terrain folder. Stage each replacement in the same directory;
/// a retry repairs a partial upgrade, and load() verifies both before use.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
pub fn install(
    dir: &Path,
    mut read_packaged: impl FnMut(&str, usize) -> io::Result<Vec<u8>>,
) -> io::Result<()> {
    for file in FILES {
        if verified_file(dir, file).is_ok() {
            continue;
        }
        let bytes = read_packaged(file.0, file.1)?;
        verify(&bytes, file)?;
        std::fs::create_dir_all(dir)?;
        crate::util::file_system::write_staged(dir, &bytes, |stage| {
            std::fs::rename(stage, dir.join(file.0))
        })?;
    }
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn install_editor_files(settings_dir: &Path) -> io::Result<()> {
    let source = terrain_directory()?;
    install(&settings_dir.join(DIRECTORY), |name, size| {
        read_bounded(&source.join(name), size)
    })
}

#[cfg(all(test, tests_assets))]
mod tests {
    use super::*;

    #[test]
    fn installation_rejects_a_changed_pack_and_repairs_a_partial_upgrade() {
        let terrain = Path::new(env!("CARGO_MANIFEST_DIR")).join("../Terrain");
        let profile = tempfile::tempdir().unwrap();
        let dir = profile.path().join(DIRECTORY);
        let [mix, ini] = FILE_NAMES;
        let mut changed = std::fs::read(terrain.join(ini)).unwrap();
        changed[0] ^= 1;
        let result = install(&dir, |name, _| {
            if name == ini {
                return Ok(changed.clone());
            }
            std::fs::read(terrain.join(name))
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert!(dir.join(mix).is_file());
        assert!(!dir.join(ini).exists());

        std::fs::write(dir.join(ini), &changed).unwrap();
        assert_eq!(
            verified_file(&dir, FILES[1]).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        install(&dir, |name, _| {
            assert_eq!(name, ini);
            std::fs::read(terrain.join(name))
        })
        .unwrap();
        install(&dir, |_, _| panic!("already up to date")).unwrap();
    }
}
