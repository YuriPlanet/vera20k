//! VERA: `Account.json` beside VERA.INI, the online account this computer
//! signs in to without its password after Remember password
//! (docs/lobby-protocol.md "Accounts"). One per lobby origin, so a test lobby
//! never receives another lobby's sign-in; on Unix only the owner may read
//! the file.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub(crate) const FILENAME: &str = "Account.json";

/// A remembered account: its name and the lobby's sign-in token.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Remembered {
    pub(crate) name: String,
    pub(crate) sign_in_token: String,
}

/// Never shows the token.
impl std::fmt::Debug for Remembered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Remembered")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// A token as the lobby issues it: 32 bytes in unpadded base64url.
pub(crate) fn valid_token(token: &str) -> bool {
    token.len() == 43
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// The remembered accounts by lobby; an unreadable file is an error, so a
/// save never replaces what it could not read.
type Accounts = BTreeMap<String, Remembered>;

fn read(directory: &Path) -> io::Result<Accounts> {
    match crate::util::file_system::read(directory.join(FILENAME)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Accounts::new()),
        Err(error) => Err(error),
    }
}

fn write(directory: &Path, accounts: &Accounts) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(accounts).map_err(io::Error::other)?;
    write_private(&directory.join(FILENAME), &bytes)
}

/// The account this computer remembers on `lobby`.
pub(crate) fn load(directory: &Path, lobby: &str) -> Option<Remembered> {
    let mut accounts = read(directory)
        .inspect_err(|error| log::warn!("Could not read {FILENAME}: {error}"))
        .ok()?;
    accounts
        .remove(lobby)
        .filter(|account| valid_token(&account.sign_in_token))
}

/// Remember `account` on `lobby`, replacing the one before.
pub(crate) fn save(directory: &Path, lobby: &str, account: Remembered) -> io::Result<()> {
    let mut accounts = read(directory)?;
    accounts.insert(lobby.to_owned(), account);
    write(directory, &accounts)
}

/// Forget the account remembered on `lobby`, returning it.
pub(crate) fn forget(directory: &Path, lobby: &str) -> io::Result<Option<Remembered>> {
    let mut accounts = read(directory)?;
    let forgotten = accounts.remove(lobby);
    if forgotten.is_some() {
        write(directory, &accounts)?;
    }
    Ok(forgotten)
}

/// Write the whole file at once, readable only by its owner on Unix, so a
/// failed write never leaves the earlier accounts half-replaced.
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

    const TOKEN: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const LOBBY: &str = "https://lobby.vera20k.org";

    fn tanya() -> Remembered {
        Remembered {
            name: "Tanya".into(),
            sign_in_token: TOKEN.into(),
        }
    }

    #[test]
    fn a_remembered_account_stays_on_its_own_lobby_until_forgotten() {
        let directory = TestDirectory::new("account");
        assert_eq!(load(directory.path(), LOBBY), None);
        save(directory.path(), LOBBY, tanya()).unwrap();
        let staging = Remembered {
            sign_in_token: TOKEN.replace('A', "C"),
            ..tanya()
        };
        save(directory.path(), "https://staging.test", staging).unwrap();
        assert_eq!(load(directory.path(), LOBBY), Some(tanya()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(directory.path().join(FILENAME))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert_eq!(forget(directory.path(), LOBBY).unwrap(), Some(tanya()));
        assert_eq!(load(directory.path(), LOBBY), None);
        assert!(load(directory.path(), "https://staging.test").is_some());
        assert!(!format!("{:?}", tanya()).contains(TOKEN));
    }

    #[test]
    fn an_unreadable_file_is_never_replaced() {
        let directory = TestDirectory::new("account-unreadable");
        directory.write(FILENAME, b"{not json");
        assert_eq!(load(directory.path(), LOBBY), None);
        assert!(save(directory.path(), LOBBY, tanya()).is_err());
        assert_eq!(
            std::fs::read(directory.path().join(FILENAME)).unwrap(),
            b"{not json"
        );
    }
}
