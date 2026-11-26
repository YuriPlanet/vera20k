//! Each mod's and map's local key-value store (RFC 10, "Storage"): under
//! the user's config directory, `wasm-storage/<mod Name or map StorageId>/`
//! (`PathsConfig::wasm_storage_dir`). Keys 1..64 printable ASCII bytes,
//! values up to 16 KiB, up to 256 keys and 256 KiB (keys and values) per
//! store; over the limit a write is `Refused` and nothing changes. An empty
//! value deletes the key.
//!
//! On disk a store is one file, `store.bin`, in its folder:
//! `VWS1`, then per key (in key order) a `u8` key length, the key, a `u32`
//! little-endian value length and the value. A file that does not read
//! back whole is ignored (the store starts empty and the next write
//! replaces it). Writes replace the file atomically: the bytes go to a
//! staged file in the same folder, which is then renamed over it.
//!
//! Persistence is write-through with a floor of [`FLUSH_INTERVAL`] between
//! file writes: a write reaches the disk at once unless the previous one
//! was less than a second ago; then it waits for the next write after
//! that, for [`Storage::flush`], or for the store's drop at the end of the
//! match. So a module that writes every render frame writes the file about
//! once a second instead of 60 times, and a crash loses at most the last
//! second of writes.
//!
//! The folder name is the key made file-system safe: its ASCII letters and
//! digits (lowercased), `-` and `_`, everything else as `_`, cut to 32
//! bytes, then `-` and the 32-bit FNV-1a of the ASCII-lowercased key in
//! hex. Names compare ASCII case-insensitively (appendix "Strings"), so
//! `Foo` and `foo` share a store, on every file system alike, and two keys
//! that only differ in replaced characters still get their own folders.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use vera20k_wasm_ext::{Invalid, Status};

use crate::util::time::{Duration, Instant};

/// Key length in bytes.
pub(crate) const MAX_KEY_BYTES: usize = 64;
/// One value in bytes.
pub(crate) const MAX_VALUE_BYTES: usize = 16 * 1024;
/// Keys per store.
pub(crate) const MAX_KEYS: usize = 256;
/// Keys and values per store, in bytes.
pub(crate) const MAX_STORE_BYTES: usize = 256 * 1024;
/// The least time between two file writes of one store.
pub(crate) const FLUSH_INTERVAL: Duration = Duration::from_secs(1);

const MAGIC: &[u8; 4] = b"VWS1";
const FILE_NAME: &str = "store.bin";
/// Readable bytes kept of a key in its folder name.
const FOLDER_PREFIX_BYTES: usize = 32;

/// One source's store.
pub(crate) struct Storage {
    /// The store file.
    path: PathBuf,
    entries: BTreeMap<String, Vec<u8>>,
    /// Bytes of all keys and values.
    bytes: usize,
    /// Changes the file does not hold yet.
    dirty: bool,
    last_flush: Option<Instant>,
}

impl Storage {
    /// The store of `key` (a mod's `[Mod] Name`, a map's
    /// `[VeraWasm] StorageId=`) under `root` (`wasm-storage/`), read from
    /// disk if it is there.
    pub(crate) fn open(root: &Path, key: &str) -> Storage {
        let path = root.join(folder_name(key)).join(FILE_NAME);
        let entries = match crate::util::file_system::read(&path) {
            Ok(bytes) => decode(&bytes).unwrap_or_else(|| {
                log::warn!(
                    "ui.wasm storage {}: unreadable, starting empty",
                    path.display()
                );
                BTreeMap::new()
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => {
                log::warn!("ui.wasm storage {}: {error}", path.display());
                BTreeMap::new()
            }
        };
        let bytes = entries.iter().map(|(k, v)| k.len() + v.len()).sum();
        Storage {
            path,
            entries,
            bytes,
            dirty: false,
            last_flush: None,
        }
    }

    /// `client_storage_read`: the value, or `None` and `NotFound` (also for
    /// a key no store can hold).
    pub(crate) fn read(&mut self, key: &str) -> Result<(Option<Vec<u8>>, Status), Invalid> {
        Ok(match self.entries.get(key) {
            Some(value) => (Some(value.clone()), Status::Ok),
            None => (None, Status::NotFound),
        })
    }

    /// `client_storage_write`: sets `key` to `data`, or with empty `data`
    /// deletes it. `Refused`, changing nothing, for a key outside 1..64
    /// printable ASCII bytes, a value over 16 KiB, or a store that would
    /// pass 256 keys or 256 KiB.
    pub(crate) fn write(&mut self, key: &str, data: &[u8]) -> Result<Status, Invalid> {
        if !valid_key(key) || data.len() > MAX_VALUE_BYTES {
            return Ok(Status::Refused);
        }
        let old = self.entries.get(key).map(|value| key.len() + value.len());
        if data.is_empty() {
            if let Some(old) = old {
                self.entries.remove(key);
                self.bytes -= old;
                self.changed();
            }
            return Ok(Status::Ok);
        }
        let bytes = self.bytes - old.unwrap_or(0) + key.len() + data.len();
        if bytes > MAX_STORE_BYTES || (old.is_none() && self.entries.len() >= MAX_KEYS) {
            return Ok(Status::Refused);
        }
        if self.entries.get(key).is_some_and(|value| value == data) {
            return Ok(Status::Ok);
        }
        self.entries.insert(key.to_owned(), data.to_vec());
        self.bytes = bytes;
        self.changed();
        Ok(Status::Ok)
    }

    fn changed(&mut self) {
        self.dirty = true;
        if self
            .last_flush
            .is_none_or(|last| last.elapsed() >= FLUSH_INTERVAL)
        {
            self.flush();
        }
    }

    /// Writes pending changes to the file. A failed write is logged and
    /// stays pending.
    pub(crate) fn flush(&mut self) {
        if !self.dirty {
            return;
        }
        self.last_flush = Some(Instant::now());
        match write_atomically(&self.path, &encode(&self.entries)) {
            Ok(()) => self.dirty = false,
            Err(error) => log::warn!("ui.wasm storage {}: {error}", self.path.display()),
        }
    }
}

impl Drop for Storage {
    fn drop(&mut self) {
        self.flush();
    }
}

/// 1..64 printable ASCII bytes.
fn valid_key(key: &str) -> bool {
    (1..=MAX_KEY_BYTES).contains(&key.len()) && key.bytes().all(|b| (0x20..=0x7E).contains(&b))
}

/// The store folder's name for `key` (see the module doc).
pub(crate) fn folder_name(key: &str) -> String {
    let lower = key.to_ascii_lowercase();
    let mut name: String = lower
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(FOLDER_PREFIX_BYTES)
        .collect();
    let digest = lower.bytes().fold(0x811C_9DC5u32, |digest, byte| {
        (digest ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    });
    name.push_str(&format!("-{digest:08x}"));
    name
}

fn encode(entries: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let size: usize = entries.iter().map(|(k, v)| 5 + k.len() + v.len()).sum();
    let mut bytes = Vec::with_capacity(MAGIC.len() + size);
    bytes.extend_from_slice(MAGIC);
    for (key, value) in entries {
        // Keys are at most 64 bytes, values at most 16 KiB (`write`).
        bytes.push(key.len() as u8);
        bytes.extend_from_slice(key.as_bytes());
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        bytes.extend_from_slice(value);
    }
    bytes
}

/// The entries of a store file; `None` for a file that is not one whole,
/// within the limits.
fn decode(bytes: &[u8]) -> Option<BTreeMap<String, Vec<u8>>> {
    let mut rest = bytes.strip_prefix(MAGIC.as_slice())?;
    let mut entries = BTreeMap::new();
    let mut total = 0;
    while let Some((&key_len, after)) = rest.split_first() {
        let (key, after) = after.split_at_checked(usize::from(key_len))?;
        let (len, after) = after.split_at_checked(4)?;
        let len = u32::from_le_bytes(len.try_into().ok()?) as usize;
        let (value, after) = after.split_at_checked(len)?;
        let key = std::str::from_utf8(key).ok()?;
        if !valid_key(key) || value.is_empty() || value.len() > MAX_VALUE_BYTES {
            return None;
        }
        total += key.len() + value.len();
        if entries.insert(key.to_owned(), value.to_vec()).is_some()
            || entries.len() > MAX_KEYS
            || total > MAX_STORE_BYTES
        {
            return None;
        }
        rest = after;
    }
    Some(entries)
}

/// Replaces `path`'s bytes whole: a staged file in the same folder renamed
/// over it, so a crash leaves the old file or the new one. The browser's
/// file system is in memory, where a write is already whole.
fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let folder = path.parent().unwrap_or(Path::new("."));
    crate::util::file_system::create_dir_all(folder)?;
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    {
        crate::util::file_system::write(path, bytes)
    }
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    {
        crate::util::file_system::write_staged(folder, bytes, |stage| std::fs::rename(stage, path))
    }
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    use crate::map::source::test_support::TestDirectory;

    #[test]
    fn values_round_trip_through_the_file() {
        let root = TestDirectory::new("wasm-storage-round-trip");
        {
            let mut store = Storage::open(root.path(), "My Mod");
            assert_eq!(store.read("best"), Ok((None, Status::NotFound)));
            assert_eq!(store.write("best", b"42"), Ok(Status::Ok));
            assert_eq!(store.write("other", b"x"), Ok(Status::Ok));
            assert_eq!(store.write("other", b""), Ok(Status::Ok));
            assert_eq!(store.write("missing", b""), Ok(Status::Ok));
        }
        let mut store = Storage::open(root.path(), "MY MOD");
        assert_eq!(store.read("best"), Ok((Some(b"42".to_vec()), Status::Ok)));
        assert_eq!(store.read("other"), Ok((None, Status::NotFound)));
    }

    #[test]
    fn quotas_refuse_and_change_nothing() {
        let root = TestDirectory::new("wasm-storage-quotas");
        let mut store = Storage::open(root.path(), "quotas");
        let long_key = "k".repeat(MAX_KEY_BYTES + 1);
        assert_eq!(store.write("", b"x"), Ok(Status::Refused));
        assert_eq!(store.write(&long_key, b"x"), Ok(Status::Refused));
        assert_eq!(store.write("tab\tkey", b"x"), Ok(Status::Refused));
        assert_eq!(store.write("ключ", b"x"), Ok(Status::Refused));
        assert_eq!(
            store.write("big", &vec![0; MAX_VALUE_BYTES + 1]),
            Ok(Status::Refused)
        );
        assert_eq!(
            store.write(&"k".repeat(MAX_KEY_BYTES), &vec![1; MAX_VALUE_BYTES]),
            Ok(Status::Ok)
        );

        // 256 KiB in all: fifteen more full values fit, the sixteenth not.
        for n in 0..15 {
            let key = format!("v{n:02}");
            assert_eq!(
                store.write(&key, &vec![1; MAX_VALUE_BYTES - 64]),
                Ok(Status::Ok)
            );
        }
        assert!(store.bytes <= MAX_STORE_BYTES);
        assert_eq!(
            store.write("v15", &vec![1; MAX_VALUE_BYTES]),
            Ok(Status::Refused)
        );
        assert_eq!(store.read("v15"), Ok((None, Status::NotFound)));
        // Replacing a value counts only the difference.
        assert_eq!(
            store.write("v00", &vec![2; MAX_VALUE_BYTES - 64]),
            Ok(Status::Ok)
        );

        // 256 keys at most.
        let mut store = Storage::open(root.path(), "keys");
        for n in 0..MAX_KEYS {
            assert_eq!(store.write(&format!("k{n}"), b"1"), Ok(Status::Ok));
        }
        assert_eq!(store.write("one more", b"1"), Ok(Status::Refused));
        assert_eq!(store.write("k0", b"2"), Ok(Status::Ok));
        assert_eq!(store.write("k0", b""), Ok(Status::Ok));
        assert_eq!(store.write("one more", b"1"), Ok(Status::Ok));
    }

    #[test]
    fn the_file_format_rejects_damage() {
        let mut entries = BTreeMap::new();
        entries.insert("a".to_owned(), vec![1, 2, 3]);
        entries.insert("b c".to_owned(), vec![4]);
        let bytes = encode(&entries);
        assert_eq!(decode(&bytes), Some(entries));
        assert_eq!(decode(MAGIC), Some(BTreeMap::new()));
        assert_eq!(decode(&bytes[..bytes.len() - 1]), None);
        assert_eq!(decode(b"VWS2"), None);
        let mut bad_key = bytes.clone();
        bad_key[5] = 0x07;
        assert_eq!(decode(&bad_key), None);
    }

    #[test]
    fn folder_names_are_safe_and_case_insensitive() {
        let name = folder_name("My Mod: Remix/2?");
        assert!(name.starts_with("my_mod__remix_2_-"));
        assert_eq!(name, folder_name("MY MOD: REMIX/2?"));
        assert_ne!(name, folder_name("My Mod: Remix/2!"));
        assert_ne!(folder_name("a b"), folder_name("a_b"));
        assert!(
            folder_name("..")
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        );
        assert!(folder_name(&"x".repeat(200)).len() <= FOLDER_PREFIX_BYTES + 9);
        assert!(folder_name("地图").starts_with("__-"));
    }
}
