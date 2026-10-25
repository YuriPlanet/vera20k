//! Filesystem quota and active-file protection for the shared network-map cache.
//!
//! The directory lock serializes cooperating processes. A returned cache file
//! stays pinned while a source or pending upload retains its shared sidecar lock;
//! eviction requires the exclusive lock, so another VERA instance cannot remove
//! a map still needed by a loader. The final source clone releases the pin.

use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::{Context, Result, ensure};

use super::{TransferResource, valid_source_sha256};

#[derive(Clone, Copy)]
pub(super) struct Limits {
    pub bytes: u64,
    pub files: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            bytes: 256 * 1024 * 1024,
            files: 128,
        }
    }
}

pub(super) struct Cache {
    directory: PathBuf,
    _lock: File,
    limits: Limits,
}

struct Entry {
    path: PathBuf,
    digest: String,
    length: u64,
    modified: SystemTime,
    resource: TransferResource,
}

/// A cache file's shared lock, retained only by its active consumers. The lock
/// is process metadata: it does not change a source's serialized identity or
/// equality, and an ordinary loose source has the empty default lease.
#[derive(Clone, Default)]
pub struct CacheLease(Option<Arc<File>>);

impl PartialEq for CacheLease {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for CacheLease {}

impl std::fmt::Debug for CacheLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CacheLease")
            .field("pinned", &self.0.is_some())
            .finish()
    }
}

fn open_regular(path: &Path, create: bool, writable: bool) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(writable).create(create);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT: inspect a link instead of its target.
        options.custom_flags(0x0020_0000);
    }
    let file = options.open(path)?;
    ensure!(
        file.metadata()?.is_file() && !fs::symlink_metadata(path)?.file_type().is_symlink(),
        "network cache entry is not a regular file"
    );
    Ok(file)
}

fn pin_path(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        ".{}.pin",
        path.file_name().expect("cache filename").to_string_lossy()
    ))
}

fn entry_name(name: &str) -> Option<(&str, TransferResource)> {
    let (digest, extension) = name.split_once('.')?;
    if !valid_source_sha256(digest) {
        return None;
    }
    let resource = match extension {
        "map" => TransferResource::Map,
        "sed" => TransferResource::Seed,
        "pcx" => TransferResource::Preview,
        _ => return None,
    };
    Some((digest, resource))
}

impl Cache {
    pub fn open(directory: &Path, create: bool, limits: Limits) -> Result<Self> {
        if create {
            fs::create_dir_all(directory)?;
        }
        let directory = fs::canonicalize(directory)?;
        let lock = open_regular(&directory.join(".vera20k-cache.lock"), true, true)?;
        lock.lock().context("lock network map cache")?;
        Ok(Self {
            directory,
            _lock: lock,
            limits,
        })
    }

    pub fn path(&self, digest: &str, resource: TransferResource) -> Result<PathBuf> {
        let path = self
            .directory
            .join(format!("{digest}.{}", resource.extension()));
        match fs::symlink_metadata(&path) {
            Ok(metadata) => ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "network cache destination is not a regular file"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(path)
    }

    pub fn pin(&self, path: &Path) -> Result<CacheLease> {
        let file = open_regular(&pin_path(path), true, true)?;
        file.lock_shared().context("pin active network map")?;
        Ok(CacheLease(Some(Arc::new(file))))
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Reserve committed files plus the temporary incoming bytes before writing.
    /// Only verified, inactive digest-named data files are evicted. Foreign
    /// files and abandoned writes count against quota but are never deleted.
    /// Links and directories are neither followed nor deleted.
    pub fn make_room(&self, destination: &Path, incoming: usize) -> Result<()> {
        ensure!(
            incoming as u64 <= self.limits.bytes,
            "network map exceeds cache quota"
        );
        let mut entries = Vec::new();
        let mut files = usize::from(incoming != 0);
        let mut bytes = incoming as u64;
        for item in fs::read_dir(&self.directory)? {
            let item = item?;
            let metadata = fs::symlink_metadata(item.path())?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                continue;
            }
            let name = item.file_name();
            let name = name.to_str().unwrap_or("");
            if metadata.len() == 0 {
                if name == ".vera20k-cache.lock" {
                    continue;
                }
                if let Some(data_name) = name.strip_prefix('.').and_then(|s| s.strip_suffix(".pin"))
                    && entry_name(data_name).is_some()
                {
                    // A crash between deleting data and retiring its pin must
                    // not accumulate sidecars across process restarts.
                    if matches!(
                        fs::symlink_metadata(self.directory.join(data_name)),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound
                    ) && let Ok(pin) = open_regular(&item.path(), false, true)
                        && pin.try_lock().is_ok()
                    {
                        fs::remove_file(item.path())?;
                    }
                    continue;
                }
            }
            files += 1;
            bytes = bytes
                .checked_add(metadata.len())
                .ok_or_else(|| anyhow::anyhow!("network cache size overflow"))?;
            let Some((digest, resource)) = entry_name(name) else {
                continue;
            };
            entries.push(Entry {
                path: item.path(),
                digest: digest.to_owned(),
                length: metadata.len(),
                modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                resource,
            });
        }
        entries.sort_by(|a, b| (a.modified, &a.path).cmp(&(b.modified, &b.path)));
        for entry in entries {
            if files <= self.limits.files && bytes <= self.limits.bytes {
                break;
            }
            if entry.path == destination || entry.length > entry.resource.limit() as u64 {
                continue;
            }
            let Ok(file) = open_regular(&entry.path, false, false) else {
                continue;
            };
            let mut contents = Vec::new();
            file.take(entry.resource.limit() as u64 + 1)
                .read_to_end(&mut contents)?;
            if contents.len() as u64 != entry.length
                || crate::util::sha256::sha256_hex(&contents) != entry.digest
            {
                continue;
            }
            let Ok(pin) = open_regular(&pin_path(&entry.path), true, true) else {
                continue;
            };
            if pin.try_lock().is_err() {
                continue;
            }
            fs::remove_file(&entry.path)?;
            // No cooperating opener can race sidecar retirement while the
            // directory lock is held and this exclusive pin lock is live.
            fs::remove_file(pin_path(&entry.path))?;
            bytes -= entry.length;
            files -= 1;
        }
        ensure!(
            files <= self.limits.files && bytes <= self.limits.bytes,
            "network map cache is full; active or unrecognized files are retained"
        );
        Ok(())
    }
}
