//! A mod folder or a private view of its ordered ZIP and directory layers.
//!
//! Keep the returned folder alive while using paths resolved within it. The
//! package files stay in place and a failed preparation publishes no files.

use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

#[derive(Debug, Clone)]
pub struct ModFolder(Storage);

#[derive(Debug, Clone)]
enum Storage {
    Loose(PathBuf),
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    Prepared(std::sync::Arc<tempfile::TempDir>),
}

impl ModFolder {
    pub fn loose(path: PathBuf) -> Self {
        Self(Storage::Loose(path))
    }

    /// Combine a package's immediate parts in a private native directory.
    /// The browser continues to use the files imported into its memory store.
    pub fn open(dir: &Path, stop: Option<&AtomicBool>) -> io::Result<Self> {
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        if let Some(prepared) = native::prepare(dir, stop)? {
            return Ok(Self(Storage::Prepared(std::sync::Arc::new(prepared))));
        }
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        let _ = stop;
        Ok(Self::loose(dir.to_path_buf()))
    }

    pub fn path(&self) -> &Path {
        match &self.0 {
            Storage::Loose(path) => path,
            #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
            Storage::Prepared(dir) => dir.path(),
        }
    }

    pub fn is_prepared(&self) -> bool {
        match &self.0 {
            Storage::Loose(_) => false,
            #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
            Storage::Prepared(_) => true,
        }
    }
}

impl Deref for ModFolder {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        self.path()
    }
}

impl PartialEq for ModFolder {
    fn eq(&self, other: &Self) -> bool {
        self.path() == other.path()
    }
}

impl Eq for ModFolder {}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
mod native {
    use std::collections::{HashMap, HashSet, hash_map::Entry};
    use std::ffi::OsString;
    use std::fs::{self, File, Metadata, OpenOptions};
    use std::io::{self, BufReader, Read, Write};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};

    use tempfile::TempDir;
    use zip::ZipReadOptions;
    use zip::read::Config;
    use zip::unstable::read::ZipIterable;

    // Bound even overwritten files and empty directories. Loose folders keep
    // their existing limits in the consumers that read them.
    const MAX_PARTS: usize = 32;
    const MAX_ENTRIES: usize = 50_000;
    const MAX_DEPTH: usize = 16;
    const MAX_BYTES: u64 = 8 * 1024 * 1024 * 1024;

    enum Kind {
        Zip,
        Directory,
    }

    struct Part {
        path: PathBuf,
        key: String,
        kind: Kind,
    }

    fn invalid(message: impl Into<String>) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, message.into())
    }

    fn check_stop(stop: Option<&AtomicBool>) -> io::Result<()> {
        if stop.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            Err(io::ErrorKind::Interrupted.into())
        } else {
            Ok(())
        }
    }

    fn ignored(name: &str) -> bool {
        name.starts_with('.')
            || ["__MACOSX", "Thumbs.db", "desktop.ini"]
                .iter()
                .any(|metadata| metadata.eq_ignore_ascii_case(name))
    }

    fn asset_directory(name: &str) -> bool {
        ["ini", "resources", "maps", "mixfiles", "taunts"]
            .iter()
            .any(|asset| asset.eq_ignore_ascii_case(name))
    }

    fn is_game_root(path: &Path) -> io::Result<bool> {
        if is_link(&fs::symlink_metadata(path)?) {
            return Ok(false);
        }
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let Some(name) = entry.file_name().into_string().ok() else {
                continue;
            };
            let kind = entry.file_type()?;
            if !ignored(&name)
                && (kind.is_dir() && asset_directory(&name)
                    || kind.is_file()
                        && Path::new(&name)
                            .extension()
                            .and_then(|ext| ext.to_str())
                            .is_some_and(|ext| {
                                [
                                    "aud", "bag", "bik", "csf", "fnt", "hva", "ini", "map", "mix",
                                    "mpr", "pal", "pcx", "png", "shp", "sno", "tem", "urb", "vpl",
                                    "vxl", "wav", "yrm",
                                ]
                                .iter()
                                .any(|asset| asset.eq_ignore_ascii_case(ext))
                            }))
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn prepare(dir: &Path, stop: Option<&AtomicBool>) -> io::Result<Option<TempDir>> {
        let mut metadata = Vec::new();
        let mut parts = Vec::new();
        let mut contains_zip = false;
        for entry in fs::read_dir(dir)? {
            check_stop(stop)?;
            let entry = entry?;
            let Ok(name) = entry.file_name().into_string() else {
                // Preserve the ordinary loader's policy for its own paths.
                return Ok(None);
            };
            if ignored(&name) {
                continue;
            }
            let path = entry.path();
            let kind = entry.file_type()?;
            let key = name.to_ascii_lowercase();
            let part = if kind.is_dir() {
                // These are standard data folders in an already expanded mod.
                if asset_directory(&name) {
                    return Ok(None);
                }
                Part {
                    path,
                    key,
                    kind: Kind::Directory,
                }
            } else if let Some(stem) = key.strip_suffix(".zip") {
                contains_zip = true;
                Part {
                    path,
                    key: stem.to_owned(),
                    kind: Kind::Zip,
                }
            } else if matches!(key.as_str(), "mod.ini" | "veraui.ini" | "mod-files.txt") {
                metadata.push((path, PathBuf::from(name)));
                continue;
            } else if !key.ends_with(".txt") && !key.ends_with(".md") {
                return Ok(None);
            } else {
                continue;
            };
            // Look through the root for ordinary loose inputs, without
            // collecting an arbitrary number of candidate layers.
            if parts.len() <= MAX_PARTS {
                parts.push(part);
            }
        }
        if !contains_zip {
            if parts.len() < 2 {
                return Ok(None);
            }
            let mut wrapped = false;
            for part in &parts {
                check_stop(stop)?;
                if is_game_root(&part.path)? {
                    wrapped = true;
                    break;
                }
            }
            if !wrapped {
                return Ok(None);
            }
        }
        if parts.len() > MAX_PARTS {
            return Err(invalid(format!("a mod can have at most {MAX_PARTS} parts")));
        }
        parts.sort_unstable_by(|left, right| left.key.cmp(&right.key));
        if let Some([left, _]) = parts
            .windows(2)
            .find(|pair| matches!(pair, [left, right] if left.key == right.key))
        {
            return Err(invalid(format!(
                "more than one mod part is named {:?}; keep one copy of each part",
                left.key
            )));
        }

        let mut builder = tempfile::Builder::new();
        builder.prefix("vera20k-mod-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(fs::Permissions::from_mode(0o700));
        }
        let prepared = builder.tempdir()?;
        let mut merge = Merge {
            target: prepared.path(),
            stop,
            paths: HashMap::new(),
            in_part: HashSet::new(),
            entries_left: MAX_ENTRIES,
            bytes_left: MAX_BYTES,
            buffer: vec![0; 128 * 1024],
        };
        for part in &parts {
            merge.in_part.clear();
            let result = match part.kind {
                Kind::Zip => merge.zip(&part.path),
                Kind::Directory => merge.directory(&part.path, Path::new(""), 0),
            };
            result.map_err(|error| {
                io::Error::new(error.kind(), format!("{}: {error}", part.path.display()))
            })?;
        }
        merge.in_part.clear();
        for (source, relative) in metadata {
            merge.entry()?;
            merge.loose_file(&source, &relative)?;
        }
        check_stop(stop)?;
        Ok(Some(prepared))
    }

    struct StagedName {
        name: OsString,
        directory: bool,
    }

    struct Merge<'a> {
        target: &'a Path,
        stop: Option<&'a AtomicBool>,
        paths: HashMap<PathBuf, StagedName>,
        in_part: HashSet<OsString>,
        entries_left: usize,
        bytes_left: u64,
        buffer: Vec<u8>,
    }

    impl Merge<'_> {
        fn entry(&mut self) -> io::Result<()> {
            check_stop(self.stop)?;
            self.entries_left = self
                .entries_left
                .checked_sub(1)
                .ok_or_else(|| invalid(format!("mod package exceeds {MAX_ENTRIES} entries")))?;
            Ok(())
        }

        fn directory(&mut self, source: &Path, base: &Path, depth: usize) -> io::Result<()> {
            if depth >= MAX_DEPTH {
                return Err(invalid(format!(
                    "mod package paths exceed {MAX_DEPTH} levels"
                )));
            }
            let metadata = fs::symlink_metadata(source)?;
            if !metadata.is_dir() || is_link(&metadata) {
                return Err(invalid(format!(
                    "{} is not a plain directory",
                    source.display()
                )));
            }
            for entry in fs::read_dir(source)? {
                self.entry()?;
                let entry = entry?;
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| invalid("mod package filename is not Unicode"))?;
                if ignored(&name) {
                    continue;
                }
                portable_component(&name)?;
                let relative = base.join(&name);
                let path = entry.path();
                let metadata = fs::symlink_metadata(&path)?;
                if is_link(&metadata) {
                    return Err(invalid(format!("mod package contains a link at {name:?}")));
                }
                if metadata.is_dir() {
                    self.destination(&relative, true)?;
                    self.directory(&path, &relative, depth + 1)?;
                } else {
                    self.loose_file(&path, &relative)?;
                }
            }
            Ok(())
        }

        fn loose_file(&mut self, source: &Path, relative: &Path) -> io::Result<()> {
            let file = regular_file(source)?;
            let size = file.metadata()?.len();
            self.write_file(relative, size, file)
        }

        fn zip(&mut self, source: &Path) -> io::Result<()> {
            // Streaming central directory entries retain duplicate names and
            // allow us to set the entry budget before collecting any paths.
            // https://docs.rs/zip/9.8.1/zip/unstable/read/struct.ZipIterable.html
            let mut zip =
                ZipIterable::try_new(BufReader::new(regular_file(source)?), Config::default())?
                    .enforce_header_consistency(true);
            if zip.len() > self.entries_left {
                return Err(invalid(format!(
                    "mod package exceeds {MAX_ENTRIES} entries"
                )));
            }
            let entries = zip.files()?;
            while let Some(entry) = entries.next() {
                self.entry()?;
                let entry = entry?;
                let mut name = entry.name()?;
                let directory = entry.is_dir();
                if directory {
                    name = std::borrow::Cow::Owned(name.trim_end_matches(['/', '\\']).to_owned());
                }
                let file_type = entry.unix_mode().unwrap_or(0) & 0o170000;
                if (directory && !matches!(file_type, 0 | 0o040000))
                    || (!directory && !matches!(file_type, 0 | 0o100000))
                    || entry.is_symlink()
                {
                    return Err(invalid(format!(
                        "mod package contains a link or special file at {name:?}"
                    )));
                }
                let Some(relative) = zip_relative(&name)? else {
                    continue;
                };
                if directory {
                    self.destination(&relative, true)?;
                    continue;
                }
                let size = entry.size();
                let data = entry.with_reader(entries.reader_mut(), ZipReadOptions::new())?;
                self.write_file(&relative, size, data)?;
            }
            Ok(())
        }

        fn write_file(
            &mut self,
            relative: &Path,
            size: u64,
            mut input: impl Read,
        ) -> io::Result<()> {
            if !self
                .in_part
                .insert(relative.as_os_str().to_ascii_lowercase())
            {
                return Err(invalid(format!(
                    "a mod part contains {} twice",
                    relative.display()
                )));
            }
            self.bytes_left = self.bytes_left.checked_sub(size).ok_or_else(|| {
                invalid(format!(
                    "mod package exceeds {} GiB unpacked",
                    MAX_BYTES >> 30
                ))
            })?;
            let (destination, new_file) = self.destination(relative, false)?;
            let mut options = OpenOptions::new();
            options.write(true);
            if new_file {
                // A target filesystem may alias two names our ASCII lookup
                // keeps distinct. Refuse that collision instead of guessing.
                options.create_new(true);
            } else {
                options.truncate(true);
            }
            let mut output = options.open(&destination)?;
            let mut remaining = size;
            loop {
                check_stop(self.stop)?;
                let count = input.read(&mut self.buffer)?;
                if count == 0 {
                    break;
                }
                remaining = remaining.checked_sub(count as u64).ok_or_else(|| {
                    invalid(format!("{} grew while being read", relative.display()))
                })?;
                let bytes = self
                    .buffer
                    .get(..count)
                    .ok_or_else(|| invalid("invalid read size"))?;
                output.write_all(bytes)?;
            }
            if remaining != 0 {
                return Err(invalid(format!("{} ended early", relative.display())));
            }
            Ok(())
        }

        /// Preserve the first spelling independently at every path level.
        /// Some consumers open relative paths from INIs literally on Linux.
        fn destination(
            &mut self,
            relative: &Path,
            leaf_is_directory: bool,
        ) -> io::Result<(PathBuf, bool)> {
            let mut destination = self.target.to_path_buf();
            let mut key = PathBuf::new();
            let mut names = relative.iter().peekable();
            let mut new_leaf = false;
            while let Some(name) = names.next() {
                key.push(name.to_ascii_lowercase());
                let directory = leaf_is_directory || names.peek().is_some();
                match self.paths.entry(key.clone()) {
                    Entry::Occupied(existing) => {
                        let staged = existing.get();
                        if staged.directory != directory {
                            return Err(invalid(format!(
                                "mod package uses {} as both a file and a directory",
                                key.display()
                            )));
                        }
                        destination.push(&staged.name);
                        new_leaf = false;
                    }
                    Entry::Vacant(slot) => {
                        destination.push(name);
                        if directory {
                            // create_dir refuses aliases our ASCII index keeps distinct.
                            fs::create_dir(&destination)?;
                        }
                        slot.insert(StagedName {
                            name: name.to_owned(),
                            directory,
                        });
                        new_leaf = true;
                    }
                }
            }
            Ok((destination, new_leaf))
        }
    }

    fn is_link(metadata: &Metadata) -> bool {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            // FILE_ATTRIBUTE_REPARSE_POINT also rejects junctions.
            metadata.file_attributes() & 0x400 != 0
        }
        #[cfg(not(windows))]
        metadata.file_type().is_symlink()
    }

    fn regular_file(path: &Path) -> io::Result<File> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file() || is_link(&metadata) {
            return Err(invalid(format!("{} is not a regular file", path.display())));
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // FILE_FLAG_OPEN_REPARSE_POINT leaves the final link unopened.
            options.custom_flags(0x0020_0000);
        }
        let file = options.open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || is_link(&metadata) {
            return Err(invalid(format!("{} is not a regular file", path.display())));
        }
        Ok(file)
    }

    fn zip_relative(name: &str) -> io::Result<Option<PathBuf>> {
        let mut result = PathBuf::new();
        let mut skip = false;
        for (depth, component) in name.split(['/', '\\']).enumerate() {
            if depth >= MAX_DEPTH {
                return Err(invalid(format!(
                    "mod package paths exceed {MAX_DEPTH} levels"
                )));
            }
            portable_component(component)?;
            result.push(component);
            skip |= ignored(component);
        }
        Ok((!skip).then_some(result))
    }

    fn portable_component(name: &str) -> io::Result<()> {
        let lower = name.to_ascii_lowercase();
        let stem = lower.split('.').next().unwrap_or_default().trim_end();
        let device = matches!(stem, "aux" | "con" | "conin$" | "conout$" | "nul" | "prn")
            || ["com", "lpt"].iter().any(|prefix| {
                stem.strip_prefix(*prefix).is_some_and(|suffix| {
                    matches!(
                        suffix,
                        "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                    )
                })
            });
        if name.is_empty()
            || name.len() > 255
            || name.ends_with(['.', ' '])
            || name.chars().any(|ch| {
                ch.is_control()
                    || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
            })
            || device
        {
            return Err(invalid(format!(
                "unsafe mod package path component {name:?}"
            )));
        }
        Ok(())
    }
}

#[cfg(all(
    test,
    tests_util,
    not(all(target_arch = "wasm32", target_os = "unknown"))
))]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;

    fn write_files(root: &Path, entries: &[(&str, &[u8])]) {
        for (name, bytes) in entries {
            let path = root.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])], method: zip::CompressionMethod) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        let options = zip::write::SimpleFileOptions::default().compression_method(method);
        for (name, bytes) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn mod_parts_assemble_the_same_files_as_extracted_folders_or_zips() {
        let extracted = tempfile::tempdir().unwrap();
        let archives = tempfile::tempdir().unwrap();
        let base: &[(&str, &[u8])] = &[
            ("Rules.INI", b"original"),
            ("Resources/Faction.png", b"base art"),
            ("mod.ini", b"[Mod]\nName=Base"),
        ];
        let patch: &[(&str, &[u8])] = &[
            ("rules.ini", b"updated"),
            ("resources/faction.PNG", b"updated art"),
            ("resources/Extras.pcx", b"new art"),
        ];
        // Create them in reverse order and vary the filename case.
        write_files(&extracted.path().join("Z Patch"), patch);
        write_files(&extracted.path().join("a Base"), base);
        write_zip(
            &archives.path().join("Z Patch.ZIP"),
            patch,
            zip::CompressionMethod::Deflated,
        );
        write_zip(
            &archives.path().join("a Base.zip"),
            &[
                ("Rules.INI", b"original"),
                ("Resources\\Faction.png", b"base art"),
                ("mod.ini", b"[Mod]\nName=Base"),
            ],
            zip::CompressionMethod::Stored,
        );

        for input in [extracted.path(), archives.path()] {
            fs::write(input.join("MOD.INI"), b"[Mod]\nName=Bundle").unwrap();
            let prepared = ModFolder::open(input, None).unwrap();
            assert!(prepared.is_prepared());
            let ready = prepared.clone();
            drop(prepared);
            assert_eq!(fs::read(ready.join("Rules.INI")).unwrap(), b"updated");
            assert_eq!(
                fs::read(ready.join("Resources/Faction.png")).unwrap(),
                b"updated art"
            );
            assert_eq!(
                fs::read(ready.join("Resources/Extras.pcx")).unwrap(),
                b"new art"
            );
            let mut asset_names: Vec<_> = fs::read_dir(ready.join("Resources"))
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            asset_names.sort_unstable();
            assert_eq!(
                asset_names,
                ["Extras.pcx", "Faction.png"].map(std::ffi::OsString::from)
            );
            assert!(
                fs::read_dir(ready.path())
                    .unwrap()
                    .any(|entry| entry.unwrap().file_name() == "Resources")
            );
            assert_eq!(
                fs::read(ready.join("mod.ini")).unwrap(),
                b"[Mod]\nName=Bundle"
            );
            assert!(!input.join("rules.ini").exists());
            let prepared_path = ready.path().to_owned();
            drop(ready);
            assert!(!prepared_path.exists());
        }
    }

    #[test]
    fn unsafe_archive_path_refuses_the_whole_package() {
        let package = tempfile::tempdir().unwrap();
        write_zip(
            &package.path().join("base.zip"),
            &[("rules.ini", b"valid"), ("../outside.ini", b"outside")],
            zip::CompressionMethod::Stored,
        );
        let error = ModFolder::open(package.path(), None).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("unsafe mod package path"));
        assert_eq!(fs::read_dir(package.path()).unwrap().count(), 1);
    }
}
