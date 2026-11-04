//! VERA: `LadderKeys.json` beside VERA.INI, the keys of this computer's
//! names on the online lobby's ranked ladder. The lobby hands a game the key
//! of a name it claims as its first ranked match begins (`timing_confirmed`),
//! and that name then signs in only with it (docs/lobby-protocol.md). One key
//! per lobby origin and lowercased name, so a test lobby never replaces or
//! receives another lobby's key; on Unix only the owner may read the file.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

pub(crate) const FILENAME: &str = "LadderKeys.json";

/// A key as the lobby issues it: 32 bytes in unpadded base64url.
pub(crate) fn valid_key(key: &str) -> bool {
    key.len() == 43
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// The saved keys; an unreadable file is an error, so a save never
/// replaces the keys it could not read.
type Keys = BTreeMap<String, BTreeMap<String, String>>;

fn read(directory: &Path) -> io::Result<Keys> {
    match crate::util::file_system::read(directory.join(FILENAME)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Keys::new()),
        Err(error) => Err(error),
    }
}

/// The key this computer holds for `name` on `lobby`.
pub(crate) fn load(directory: &Path, lobby: &str, name: &str) -> Option<String> {
    let mut keys = read(directory)
        .inspect_err(|error| log::warn!("Could not read {FILENAME}: {error}"))
        .ok()?;
    keys.get_mut(lobby)?
        .remove(&name.to_ascii_lowercase())
        .filter(|key| valid_key(key))
}

/// Keep `key` for `name` on `lobby`, replacing an earlier one.
pub(crate) fn save(directory: &Path, lobby: &str, name: &str, key: &str) -> io::Result<()> {
    let mut keys = read(directory)?;
    keys.entry(lobby.to_owned())
        .or_default()
        .insert(name.to_ascii_lowercase(), key.to_owned());
    let bytes = serde_json::to_vec_pretty(&keys).map_err(io::Error::other)?;
    write_private(&directory.join(FILENAME), &bytes)
}

/// Write the whole file at once, readable only by its owner on Unix, so a
/// failed write never leaves the earlier keys half-replaced.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let temporary = path.with_extension("json.tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temporary, path)
}

#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    crate::util::file_system::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::source::test_support::TestDirectory;

    const KEY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const LOBBY: &str = "https://lobby.vera20k.org";

    #[test]
    fn a_saved_key_signs_in_its_name_in_any_case_on_its_own_lobby() {
        let directory = TestDirectory::new("ladder-keys");
        assert_eq!(load(directory.path(), LOBBY, "Tanya"), None);
        save(directory.path(), LOBBY, "Tanya", KEY).unwrap();
        save(directory.path(), LOBBY, "Boris", &KEY.replace('A', "B")).unwrap();
        save(
            directory.path(),
            "https://staging.test",
            "Tanya",
            &KEY.replace('A', "C"),
        )
        .unwrap();
        assert_eq!(load(directory.path(), LOBBY, "TANYA").as_deref(), Some(KEY));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(directory.path().join(FILENAME))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn an_unreadable_file_is_never_replaced() {
        let directory = TestDirectory::new("ladder-keys-unreadable");
        directory.write(FILENAME, b"{not json");
        assert_eq!(load(directory.path(), LOBBY, "Tanya"), None);
        assert!(save(directory.path(), LOBBY, "Tanya", KEY).is_err());
        assert_eq!(
            std::fs::read(directory.path().join(FILENAME)).unwrap(),
            b"{not json"
        );
    }
}
