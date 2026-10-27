//! Filesystem quota and active-file protection for the shared network-map cache.
//!
//! The directory lock serializes cooperating processes. A returned cache file
//! stays pinned while a source or pending upload retains its shared sidecar lock;
//! eviction requires the exclusive lock, so another VERA instance cannot remove
//! a map still needed by a loader. The final source clone releases the pin.
//! Browser networking uses the same quota/eviction owner over the per-tab VFS;
//! synchronous operations need no process lock, and shared leases retain pins.

#[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
use crate::util::file_system as fs;
#[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
use crate::util::time::SystemTime;
#[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
use std::fs::{self, File, OpenOptions};
#[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
use std::time::SystemTime;

#[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
use anyhow::Context;
use anyhow::{Result, ensure};

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
    #[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
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
pub struct CacheLease(Option<Arc<Lease>>);

#[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
type Lease = File;
#[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
type Lease = PathBuf;

#[cfg(any(
    all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"),
    all(test, feature = "wasm-net")
))]
mod browser_pins;

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

#[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
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

#[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
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
        #[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
        let lock = open_regular(&directory.join(".vera20k-cache.lock"), true, true)?;
        #[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
        lock.lock().context("lock network map cache")?;
        Ok(Self {
            directory,
            #[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
            _lock: lock,
            limits,
        })
    }

    pub fn path(&self, digest: &str, resource: TransferResource) -> Result<PathBuf> {
        let path = self
            .directory
            .join(format!("{digest}.{}", resource.extension()));
        match entry_metadata(&path) {
            Ok(metadata) => ensure!(
                is_regular(&metadata),
                "network cache destination is not a regular file"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(path)
    }

    pub fn pin(&self, path: &Path) -> Result<CacheLease> {
        #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
        {
            // Browser cache operations are synchronous on this tab's one
            // event loop. Only active-file leases need to outlive the call.
            Ok(CacheLease(Some(browser_pins::pin(path))))
        }
        #[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
        {
            let file = open_regular(&pin_path(path), true, true)?;
            file.lock_shared().context("pin active network map")?;
            Ok(CacheLease(Some(Arc::new(file))))
        }
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
            let metadata = entry_metadata(&item.path())?;
            if !is_regular(&metadata) {
                continue;
            }
            let name = item.file_name();
            let name = name.to_str().unwrap_or("");
            if metadata.len() == 0 {
                if name == ".vera20k-cache.lock" {
                    continue;
                }
                #[cfg(not(all(
                    feature = "wasm-net",
                    target_arch = "wasm32",
                    target_os = "unknown"
                )))]
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
            let contents = read_eviction_candidate(&entry.path, entry.resource.limit())?;
            let Some(contents) = contents else {
                continue;
            };
            if contents.len() as u64 != entry.length
                || crate::util::sha256::sha256_hex(&contents) != entry.digest
            {
                continue;
            }
            #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
            if browser_pins::is_pinned(&entry.path) {
                continue;
            }
            #[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
            let Ok(pin) = open_regular(&pin_path(&entry.path), true, true) else {
                continue;
            };
            #[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
            if pin.try_lock().is_err() {
                continue;
            }
            fs::remove_file(&entry.path)?;
            // No cooperating opener can race sidecar retirement while the
            // directory lock is held and this exclusive pin lock is live.
            #[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
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

// Only filesystem operations differ. Selection, quota accounting, validation,
// eviction order and active-source lifetime stay in the shared Cache owner.
#[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
fn entry_metadata(path: &Path) -> std::io::Result<fs::Metadata> {
    fs::symlink_metadata(path)
}
#[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
fn entry_metadata(path: &Path) -> std::io::Result<fs::Metadata> {
    // Imported browser files cannot be symlinks.
    fs::metadata(path)
}
#[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
fn is_regular(metadata: &fs::Metadata) -> bool {
    metadata.is_file() && !metadata.file_type().is_symlink()
}
#[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
fn is_regular(metadata: &fs::Metadata) -> bool {
    metadata.is_file()
}
#[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
fn read_eviction_candidate(path: &Path, limit: usize) -> Result<Option<Vec<u8>>> {
    let Ok(file) = open_regular(path, false, false) else {
        return Ok(None);
    };
    let mut contents = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut contents)?;
    Ok(Some(contents))
}
#[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
fn read_eviction_candidate(path: &Path, limit: usize) -> Result<Option<Arc<[u8]>>> {
    // Retain shared imported bytes without another whole-map allocation.
    Ok(fs::read_shared(path)
        .ok()
        .filter(|bytes| bytes.len() <= limit))
}
