//! Host file access. Web imports user-selected files into memory before startup.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
pub use std::fs::*;

#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
pub fn is_file(path: impl AsRef<std::path::Path>) -> bool {
    path.as_ref().is_file()
}
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
pub fn is_dir(path: impl AsRef<std::path::Path>) -> bool {
    path.as_ref().is_dir()
}
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
pub fn exists(path: impl AsRef<std::path::Path>) -> bool {
    path.as_ref().exists()
}
#[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
pub fn try_exists(path: impl AsRef<std::path::Path>) -> std::io::Result<bool> {
    path.as_ref().try_exists()
}

#[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
pub use memory::*;

#[cfg(any(
    all(test, feature = "wasm"),
    all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")
))]
mod memory {
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    use crate::util::time::SystemTime;
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    use std::time::SystemTime;
    use std::{
        collections::BTreeMap,
        io,
        path::{Component, Path, PathBuf},
        sync::{Arc, Mutex, OnceLock},
    };
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    type PersistenceCallbacks = (fn(&str, &[u8], u64), fn(&str));
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    static PERSISTENCE: OnceLock<PersistenceCallbacks> = OnceLock::new();
    /// The browser app installs its storage bridge before any engine writes.
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    pub fn set_persistence_callbacks(write: fn(&str, &[u8], u64), remove: fn(&str)) {
        let _ = PERSISTENCE.set((write, remove));
    }
    #[derive(Clone)]
    struct Entry {
        bytes: Arc<[u8]>,
        modified_ms: u64,
    }
    static FILES: OnceLock<Mutex<BTreeMap<PathBuf, Entry>>> = OnceLock::new();
    fn files() -> &'static Mutex<BTreeMap<PathBuf, Entry>> {
        FILES.get_or_init(Default::default)
    }
    fn key(path: &Path) -> PathBuf {
        let mut result = PathBuf::from("/");
        for component in path.components() {
            match component {
                Component::Normal(part) => result.push(part),
                Component::ParentDir => {
                    result.pop();
                }
                _ => {}
            }
        }
        result
    }
    static DIRECTORIES: OnceLock<Mutex<std::collections::BTreeSet<PathBuf>>> = OnceLock::new();
    fn directories() -> &'static Mutex<std::collections::BTreeSet<PathBuf>> {
        DIRECTORIES.get_or_init(|| Mutex::new([PathBuf::from("/")].into()))
    }
    pub fn is_file(path: impl AsRef<Path>) -> bool {
        metadata(path).is_ok_and(|m| m.is_file())
    }
    pub fn is_dir(path: impl AsRef<Path>) -> bool {
        metadata(path).is_ok_and(|m| m.is_dir())
    }
    pub fn exists(path: impl AsRef<Path>) -> bool {
        metadata(path).is_ok()
    }
    pub fn try_exists(path: impl AsRef<Path>) -> io::Result<bool> {
        match metadata(path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }
    // Network cache pins use the same normalized identity as stored bytes.
    #[cfg(feature = "wasm-net")]
    pub fn canonicalize(path: impl AsRef<Path>) -> io::Result<PathBuf> {
        let path = key(path.as_ref());
        metadata(&path)?;
        Ok(path)
    }
    fn missing() -> io::Error {
        io::Error::new(io::ErrorKind::NotFound, "file was not imported")
    }
    /// Import bytes before loading the game. Paths use the selected directory layout.
    pub fn import_file(path: impl AsRef<Path>, bytes: Vec<u8>) {
        restore_file(path, bytes, 0);
    }
    pub fn restore_file(path: impl AsRef<Path>, bytes: Vec<u8>, modified_ms: u64) {
        files().lock().unwrap().insert(
            key(path.as_ref()),
            Entry {
                bytes: bytes.into(),
                modified_ms,
            },
        );
    }
    pub fn read_shared(path: impl AsRef<Path>) -> io::Result<Arc<[u8]>> {
        files()
            .lock()
            .unwrap()
            .get(&key(path.as_ref()))
            .map(|entry| entry.bytes.clone())
            .ok_or_else(missing)
    }
    pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        Ok(read_shared(path)?.to_vec())
    }
    pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
        String::from_utf8(read(path)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
    pub fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> io::Result<()> {
        put(path.as_ref(), bytes.as_ref(), false)
    }
    pub fn write_new(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> io::Result<()> {
        put(path.as_ref(), bytes.as_ref(), true)
    }
    fn put(path: &Path, bytes: &[u8], exclusive: bool) -> io::Result<()> {
        let path = key(path);
        if is_dir(&path) {
            return Err(io::Error::new(
                io::ErrorKind::IsADirectory,
                "path is a directory",
            ));
        }
        if !path.parent().is_some_and(is_dir) {
            return Err(missing());
        }
        let modified_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        {
            let mut files = files().lock().unwrap();
            if exclusive && files.contains_key(&path) {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "file already exists",
                ));
            }
            files.insert(
                path.clone(),
                Entry {
                    bytes: bytes.into(),
                    modified_ms,
                },
            );
        }
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        if let Some((write, _)) = PERSISTENCE.get() {
            write(&path.to_string_lossy(), bytes, modified_ms);
        }
        Ok(())
    }
    pub fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
        let path = key(path.as_ref());
        if path
            .ancestors()
            .any(|p| files().lock().unwrap().contains_key(p))
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "path is a file",
            ));
        }
        directories()
            .lock()
            .unwrap()
            .extend(path.ancestors().map(Path::to_path_buf));
        Ok(())
    }
    pub fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
        let path = key(path.as_ref());
        files().lock().unwrap().remove(&path).ok_or_else(missing)?;
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        if let Some((_, remove)) = PERSISTENCE.get() {
            remove(&path.to_string_lossy());
        }
        Ok(())
    }
    pub struct Metadata {
        file: bool,
        len: u64,
        modified_ms: u64,
    }
    impl Metadata {
        pub fn is_file(&self) -> bool {
            self.file
        }
        pub fn is_dir(&self) -> bool {
            !self.file
        }
        pub fn len(&self) -> u64 {
            self.len
        }
        pub fn modified(&self) -> io::Result<SystemTime> {
            Ok(SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(self.modified_ms))
        }
    }
    pub fn metadata(path: impl AsRef<Path>) -> io::Result<Metadata> {
        let path = key(path.as_ref());
        let files = files().lock().unwrap();
        if let Some(bytes) = files.get(&path) {
            return Ok(Metadata {
                file: true,
                len: bytes.bytes.len() as u64,
                modified_ms: bytes.modified_ms,
            });
        }
        if directories().lock().unwrap().contains(&path)
            || files.keys().any(|p| p.starts_with(&path))
        {
            return Ok(Metadata {
                file: false,
                len: 0,
                modified_ms: 0,
            });
        }
        Err(missing())
    }
    pub struct DirEntry {
        path: PathBuf,
    }
    impl DirEntry {
        pub fn path(&self) -> PathBuf {
            self.path.clone()
        }
        pub fn file_name(&self) -> std::ffi::OsString {
            self.path.file_name().unwrap().to_os_string()
        }
        pub fn metadata(&self) -> io::Result<Metadata> {
            metadata(&self.path)
        }
        pub fn file_type(&self) -> io::Result<Metadata> {
            self.metadata()
        }
    }
    pub type ReadDir = std::vec::IntoIter<io::Result<DirEntry>>;
    pub fn read_dir(path: impl AsRef<Path>) -> io::Result<ReadDir> {
        let path = key(path.as_ref());
        if !metadata(&path)?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                "not a directory",
            ));
        }
        let mut entries: std::collections::BTreeSet<_> = files()
            .lock()
            .unwrap()
            .keys()
            .filter_map(|p| {
                let relative = p.strip_prefix(&path).ok()?;
                Some(path.join(relative.components().next()?.as_os_str()))
            })
            .collect();
        entries.extend(directories().lock().unwrap().iter().filter_map(|p| {
            let relative = p.strip_prefix(&path).ok()?;
            Some(path.join(relative.components().next()?.as_os_str()))
        }));
        Ok(entries
            .into_iter()
            .map(|path| Ok(DirEntry { path }))
            .collect::<Vec<_>>()
            .into_iter())
    }
}

#[cfg(all(test, feature = "wasm"))]
mod tests {
    use super::memory;

    #[test]
    fn imported_files_preserve_nested_directory_layout_and_shared_bytes() {
        let root = "/web-fs-layout-test";
        memory::import_file(format!("{root}/RA2MD.MIX"), vec![1, 2, 3]);
        memory::import_file(format!("{root}/Taunts/TAUAM01.WAV"), vec![4]);
        let entries: Vec<_> = memory::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap())
            .collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].file_name(), "RA2MD.MIX");
        assert!(entries[0].metadata().unwrap().is_file());
        assert_eq!(entries[1].file_name(), "Taunts");
        assert!(entries[1].metadata().unwrap().is_dir());
        assert_eq!(
            memory::read(format!("{root}/Taunts/TAUAM01.WAV")).unwrap(),
            [4]
        );
        let first = memory::read_shared(format!("{root}/RA2MD.MIX")).unwrap();
        let second = memory::read_shared(format!("{root}/RA2MD.MIX")).unwrap();
        assert!(std::sync::Arc::ptr_eq(&first, &second));
        #[cfg(feature = "wasm-net")]
        {
            assert_eq!(
                memory::canonicalize(format!("{root}/Taunts/../RA2MD.MIX")).unwrap(),
                std::path::PathBuf::from(format!("{root}/RA2MD.MIX"))
            );
            assert!(memory::canonicalize(format!("{root}/missing.map")).is_err());
        }
    }

    #[test]
    fn missing_import_and_invalid_utf8_report_file_errors() {
        let empty = "/web-fs-errors-test/empty/nested";
        assert_eq!(
            memory::write(format!("{empty}/file"), b"x")
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::NotFound
        );
        memory::create_dir_all(empty).unwrap();
        assert!(memory::is_dir(empty));
        assert_eq!(memory::read_dir(empty).unwrap().count(), 0);
        memory::write_new(format!("{empty}/file"), b"x").unwrap();
        assert_eq!(memory::read(format!("{empty}/file")).unwrap(), b"x");
        assert_eq!(
            memory::write(empty, b"x").unwrap_err().kind(),
            std::io::ErrorKind::IsADirectory
        );
        assert_eq!(
            memory::read("/web-fs-errors-test/missing")
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::NotFound
        );
        memory::import_file("/web-fs-errors-test/invalid.ini", vec![255]);
        assert_eq!(
            memory::read_to_string("/web-fs-errors-test/invalid.ini")
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData
        );
    }
    #[test]
    fn new_save_collision_preserves_bytes_and_restored_modification_time() {
        let path = "/web-save-regression/slot.sav";
        memory::restore_file(path, b"old".to_vec(), 1234);
        assert_eq!(
            memory::write_new(path, b"new").unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert_eq!(memory::read(path).unwrap(), b"old");
        assert_eq!(
            memory::metadata(path)
                .unwrap()
                .modified()
                .unwrap()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
            1234
        );
        memory::write(path, b"replacement").unwrap();
        assert_eq!(memory::read(path).unwrap(), b"replacement");
        assert!(
            memory::metadata(path).unwrap().modified().unwrap() > std::time::SystemTime::UNIX_EPOCH
        );
        memory::remove_file(path).unwrap();
        assert!(memory::read(path).is_err());
    }
}
