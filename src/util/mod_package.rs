//! A mod folder or a private view of its ordered ZIP and directory layers.
//!
//! Keep the returned folder alive while using paths resolved within it. The
//! package files stay in place and a failed preparation publishes no files.

// Outside data reaches this module: it refuses bad input, never panics.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable
    )
)]

use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

#[cfg(any(
    windows,
    target_os = "linux",
    target_os = "macos",
    target_os = "android"
))]
pub use native::import::{ModImport, PreparedModAppend, PreparedModImport};

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

    #[derive(Debug)]
    enum Kind {
        Zip,
        Directory,
    }

    #[derive(Debug)]
    struct Part {
        path: PathBuf,
        name: String,
        kind: Kind,
    }

    impl Part {
        fn new(path: PathBuf, name: &str, kind: Kind) -> Self {
            // A caller only identifies a ZIP part after checking its suffix.
            let name = match kind {
                Kind::Zip => name.get(..name.len().saturating_sub(4)).unwrap_or(name),
                Kind::Directory => name,
            };
            Self {
                path,
                name: name.to_owned(),
                kind,
            }
        }
    }

    fn check_part_count(count: usize) -> io::Result<()> {
        if count > MAX_PARTS {
            return Err(invalid(format!("a mod can have at most {MAX_PARTS} parts")));
        }
        Ok(())
    }

    fn check_part_names<'a>(parts: impl IntoIterator<Item = &'a Part>) -> io::Result<()> {
        let mut names = HashSet::new();
        for part in parts {
            if !names.insert(part.name.to_ascii_lowercase()) {
                return Err(invalid(format!(
                    "more than one mod part is named {:?}. Keep one copy of each part",
                    part.name
                )));
            }
        }
        Ok(())
    }

    fn sort_parts(parts: &mut [Part]) {
        parts.sort_by_cached_key(|part| part.name.to_ascii_lowercase());
    }

    fn order_parts(parts: &mut [Part]) -> io::Result<()> {
        check_part_count(parts.len())?;
        check_part_names(parts.iter())?;
        sort_parts(parts);
        Ok(())
    }

    fn staging_directory(prefix: &'static str) -> tempfile::Builder<'static, 'static> {
        let mut builder = tempfile::Builder::new();
        builder.prefix(prefix);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(fs::Permissions::from_mode(0o700));
        }
        builder
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
        // `hd_art`: an HD art pack (`render::hd_art`), alone or in a mod.
        ["ini", "resources", "maps", "mixfiles", "taunts", "hd_art"]
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
                                    "mpr", "pal", "pcx", "pkt", "png", "shp", "sno", "tem", "urb",
                                    "vpl", "vxl", "wav", "yrm",
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

    struct Package {
        parts: Vec<Part>,
        metadata: Vec<(PathBuf, PathBuf)>,
    }

    fn package(dir: &Path, stop: Option<&AtomicBool>) -> io::Result<Option<Package>> {
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
                Part::new(path, &name, Kind::Directory)
            } else if key.ends_with(".zip") {
                contains_zip = true;
                Part::new(path, &name, Kind::Zip)
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
        order_parts(&mut parts)?;
        Ok(Some(Package { parts, metadata }))
    }

    impl Package {
        fn expand(&self, merge: &mut Merge<'_>) -> io::Result<()> {
            merge.apply(&self.parts)?;
            merge.in_part.clear();
            for (source, relative) in &self.metadata {
                merge.entry()?;
                merge.loose_file(source, relative)?;
            }
            check_stop(merge.stop)
        }
    }

    pub(super) fn prepare(dir: &Path, stop: Option<&AtomicBool>) -> io::Result<Option<TempDir>> {
        let Some(package) = package(dir, stop)? else {
            return Ok(None);
        };
        let prepared = staging_directory("vera20k-mod-").tempdir()?;
        package.expand(&mut Merge::new(prepared.path(), stop))?;
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

    impl<'a> Merge<'a> {
        fn new(target: &'a Path, stop: Option<&'a AtomicBool>) -> Self {
            Self {
                target,
                stop,
                paths: HashMap::new(),
                in_part: HashSet::new(),
                entries_left: MAX_ENTRIES,
                bytes_left: MAX_BYTES,
                buffer: vec![0; 128 * 1024],
            }
        }

        fn apply(&mut self, parts: &[Part]) -> io::Result<()> {
            for part in parts {
                check_stop(self.stop)?;
                self.in_part.clear();
                let result = match part.kind {
                    Kind::Zip => self.zip(&part.path),
                    Kind::Directory => self.directory(&part.path, Path::new(""), 0),
                };
                result.map_err(|error| {
                    io::Error::new(error.kind(), format!("{}: {error}", part.path.display()))
                })?;
            }
            Ok(())
        }

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

    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "macos",
        target_os = "android"
    ))]
    pub(super) mod import {
        use super::*;

        /// The same ordered parts the Add Mod dialog shows and imports.
        #[derive(Debug)]
        pub struct ModImport {
            parts: Vec<Part>,
            manual_order: bool,
        }

        /// The assembled files wait here until the player accepts their install.
        #[derive(Debug)]
        pub struct PreparedModImport {
            staged: TempDir,
            name: String,
        }

        /// A replacement tree or a final package layer, prepared under the same
        /// import lock as a new installation. Only the still-open dialog publishes it.
        #[derive(Debug)]
        pub struct PreparedModAppend {
            staged: TempDir,
            target: PathBuf,
            package_layer: Option<OsString>,
            root_modified: std::time::SystemTime,
            files: usize,
            _lock: File,
        }

        fn read_part(path: &Path) -> io::Result<Part> {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| invalid(format!("{} needs a Unicode filename", path.display())))?;
            let metadata = fs::symlink_metadata(path)?;
            let zip = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"));
            let kind = if !is_link(&metadata) && metadata.is_dir() {
                Kind::Directory
            } else if !is_link(&metadata) && metadata.is_file() && zip {
                Kind::Zip
            } else {
                return Err(invalid(format!(
                    "choose a ZIP archive or an extracted mod folder: {}",
                    path.display()
                )));
            };
            Ok(Part::new(path.to_owned(), name, kind))
        }

        impl ModImport {
            pub fn plan(sources: &[PathBuf]) -> io::Result<Self> {
                if sources.is_empty() {
                    return Err(invalid("choose a ZIP archive or a mod folder"));
                }
                check_part_count(sources.len())?;
                let mut parts = sources
                    .iter()
                    .map(|path| read_part(path))
                    .collect::<io::Result<Vec<_>>>()?;
                order_parts(&mut parts)?;
                Ok(Self {
                    parts,
                    manual_order: false,
                })
            }

            /// Add a selection as a batch. Once the player rearranges the list,
            /// only the new parts are sorted and they follow the existing parts.
            pub fn add_paths(&mut self, sources: &[PathBuf]) -> io::Result<bool> {
                let mut added = Vec::new();
                for path in sources {
                    if self
                        .parts
                        .iter()
                        .chain(&added)
                        .any(|part| &part.path == path)
                    {
                        continue;
                    }
                    check_part_count(self.parts.len() + added.len() + 1)?;
                    added.push(read_part(path)?);
                }
                if added.is_empty() {
                    return Ok(false);
                }
                check_part_names(self.parts.iter().chain(&added))?;
                let sort_from = if self.manual_order {
                    self.parts.len()
                } else {
                    0
                };
                self.parts.extend(added);
                sort_parts(&mut self.parts[sort_from..]);
                Ok(true)
            }

            /// Move a part to its final index in the order used for installation.
            pub fn move_part(&mut self, from: usize, to: usize) -> bool {
                if from == to || from >= self.parts.len() || to >= self.parts.len() {
                    return false;
                }
                let part = self.parts.remove(from);
                self.parts.insert(to, part);
                self.manual_order = true;
                true
            }

            pub fn parts(&self) -> impl Iterator<Item = &Path> {
                self.parts.iter().map(|part| part.path.as_path())
            }

            pub fn name(&self) -> &str {
                let name = self.parts.first().map_or("", |part| part.name.trim());
                // Keep room for a number if the same name is already installed.
                let name = name.get(..name.floor_char_boundary(245)).unwrap_or(name);
                if ignored(name) || portable_component(name).is_err() {
                    "Imported Mod"
                } else {
                    name
                }
            }

            pub fn prepare(
                self,
                mods_dir: &Path,
                stop: &AtomicBool,
            ) -> io::Result<PreparedModImport> {
                self.stage(mods_dir, stop, true)
            }

            pub fn prepare_append(
                self,
                mods_dir: &Path,
                folder: &str,
                stop: &AtomicBool,
            ) -> io::Result<PreparedModAppend> {
                let mut components = Path::new(folder).components();
                if !matches!(components.next(), Some(std::path::Component::Normal(_)))
                    || components.next().is_some()
                    || folder.contains(['/', '\\'])
                {
                    return Err(invalid("choose an installed mod folder"));
                }
                let incoming = self.stage(mods_dir, stop, false)?;
                let mods = incoming.mods_dir()?;
                let lock = install_lock(mods)?;
                let target = mods.join(folder);
                plain_directory(&target)?;
                let root_modified = fs::symlink_metadata(&target)?.modified()?;
                let stop = Some(stop);
                let (staged, package_layer, files) = if let Some(package) = package(&target, stop)?
                {
                    // Keep the original packaged sources in place. A new ordinary
                    // directory part ordered after all of them is a single rename.
                    check_part_count(package.parts.len() + 1)?;
                    let last = package
                        .parts
                        .last()
                        .ok_or_else(|| invalid("empty package"))?;
                    let name = format!("{} + Parts", last.name);
                    portable_component(&name)?;
                    if crate::util::case_insensitive_path::find(&target, &name).is_some() {
                        return Err(invalid(
                            "no available name for another part; rename the last package part and try again",
                        ));
                    }
                    let validation = staging_directory("vera20k-mod-").tempdir()?;
                    let mut merge = Merge::new(validation.path(), stop);
                    package.expand(&mut merge)?;
                    if merge.paths.contains_key(Path::new("mod.ini")) {
                        preserve_manifest(incoming.staged.path())?;
                    }
                    for (_, relative) in &package.metadata {
                        if let Some(name) = relative.to_str()
                            && crate::util::case_insensitive_path::find(
                                incoming.staged.path(),
                                name,
                            )
                            .is_some()
                        {
                            return Err(invalid(format!(
                                "The package already overrides {name} in its top folder. Move that override into an earlier part before adding these files."
                            )));
                        }
                    }
                    // Prove the new layered package still fits the loader's limits.
                    merge.in_part.clear();
                    merge.directory(incoming.staged.path(), Path::new(""), 0)?;
                    (
                        incoming.staged,
                        Some(OsString::from(name)),
                        merge.in_part.len(),
                    )
                } else {
                    let staged = staging_directory(".vera20k-import-").tempdir_in(mods)?;
                    let mut originals = Vec::new();
                    snapshot(&target, staged.path(), 0, stop, &mut originals)?;
                    if crate::util::case_insensitive_path::find(staged.path(), "mod.ini").is_some()
                    {
                        preserve_manifest(incoming.staged.path())?;
                    }
                    let files = overlay(incoming.staged.path(), staged.path(), stop)?;
                    fs::set_permissions(staged.path(), fs::metadata(&target)?.permissions())?;
                    // Editors commonly save by replacing a file. Check the containing
                    // directories on the worker before handing the candidate to the UI.
                    for (path, modified) in originals {
                        check_stop(stop)?;
                        check_original(&path, modified)?;
                    }
                    (staged, None, files)
                };
                check_stop(stop)?;
                Ok(PreparedModAppend {
                    staged,
                    target,
                    package_layer,
                    root_modified,
                    files,
                    _lock: lock,
                })
            }

            fn stage(
                self,
                mods_dir: &Path,
                stop: &AtomicBool,
                standalone: bool,
            ) -> io::Result<PreparedModImport> {
                let stop = Some(stop);
                check_stop(stop)?;
                fs::create_dir_all(mods_dir)?;
                let mods_dir = fs::canonicalize(mods_dir)?;
                for part in &self.parts {
                    check_stop(stop)?;
                    if matches!(part.kind, Kind::Directory)
                        && mods_dir.starts_with(fs::canonicalize(&part.path)?)
                    {
                        return Err(invalid(
                            "choose the mod's folder outside the Mods folder itself",
                        ));
                    }
                }
                // Stage next to the install so publishing is a single rename.
                let staged = staging_directory(".vera20k-import-").tempdir_in(mods_dir)?;
                Merge::new(staged.path(), stop).apply(&self.parts)?;
                if standalone && !is_game_root(staged.path())? {
                    return Err(invalid(
                        "No game files found at the top level. Choose the folder that contains the mod's game files",
                    ));
                }
                check_stop(stop)?;
                Ok(PreparedModImport {
                    staged,
                    name: self.name().to_owned(),
                })
            }
        }

        impl PreparedModImport {
            fn mods_dir(&self) -> io::Result<&Path> {
                self.staged
                    .path()
                    .parent()
                    .ok_or_else(|| invalid("missing Mods folder"))
            }

            pub fn install(self) -> io::Result<String> {
                let mods_dir = self.mods_dir()?;
                let _lock = install_lock(mods_dir)?;
                for number in 1..=999 {
                    let folder = if number == 1 {
                        self.name.clone()
                    } else {
                        format!("{} {number}", self.name)
                    };
                    if crate::util::case_insensitive_path::find(mods_dir, &folder).is_some() {
                        continue;
                    }
                    // Exclusive rename also protects a folder another program
                    // may have made between our lookup and publication.
                    match rename_new(self.staged.path(), &mods_dir.join(&folder)) {
                        Ok(()) => {
                            let _ = self.staged.keep();
                            return Ok(folder);
                        }
                        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                        Err(error) => return Err(error),
                    }
                }
                Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "every mod folder name is taken",
                ))
            }
        }

        fn install_lock(mods_dir: &Path) -> io::Result<File> {
            let lock = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(mods_dir.join(".vera20k-import.lock"))?;
            match lock.try_lock() {
                Ok(()) => {}
                Err(fs::TryLockError::WouldBlock) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "another copy of VERA20K is adding a mod; try again",
                    ));
                }
                Err(fs::TryLockError::Error(error)) => return Err(error),
            }
            Ok(lock)
        }

        fn plain_directory(path: &Path) -> io::Result<()> {
            let metadata = fs::symlink_metadata(path)?;
            if is_link(&metadata) || !metadata.is_dir() {
                return Err(invalid(format!(
                    "{} is not a plain mod directory",
                    path.display()
                )));
            }
            Ok(())
        }

        fn preserve_manifest(incoming: &Path) -> io::Result<()> {
            if let Some(path) = crate::util::case_insensitive_path::find(incoming, "mod.ini") {
                // A directory masquerading as the manifest is a conflict.
                fs::remove_file(path)?;
            }
            Ok(())
        }

        /// Snapshot installed files without writing through hard links. Unrelated
        /// files, including the Android shortcut id and the player's hidden files,
        /// stay in the replacement. Filesystems without hard links use copies.
        fn snapshot(
            from: &Path,
            into: &Path,
            depth: usize,
            stop: Option<&AtomicBool>,
            originals: &mut Vec<(PathBuf, std::time::SystemTime)>,
        ) -> io::Result<()> {
            originals.push((from.to_owned(), fs::symlink_metadata(from)?.modified()?));
            if depth >= MAX_DEPTH {
                return Err(invalid(
                    "installed mod exceeds the supported directory depth",
                ));
            }
            for entry in fs::read_dir(from)? {
                check_stop(stop)?;
                let entry = entry?;
                let source = entry.path();
                let target = into.join(entry.file_name());
                let metadata = fs::symlink_metadata(&source)?;
                if is_link(&metadata) || !(metadata.is_dir() || metadata.is_file()) {
                    return Err(invalid(format!(
                        "{} is a link or special file",
                        source.display()
                    )));
                }
                if metadata.is_dir() {
                    fs::create_dir(&target)?;
                    snapshot(&source, &target, depth + 1, stop, originals)?;
                    fs::set_permissions(&target, metadata.permissions())?;
                } else if fs::hard_link(&source, &target).is_err() {
                    // In-place edits follow a hard link, but not a copied file.
                    originals.push((source.clone(), metadata.modified()?));
                    fs::copy(&source, &target)?;
                }
            }
            Ok(())
        }

        /// Install one staged part into a private snapshot, preserving the first
        /// spelling in byte order used by case_insensitive_path::find.
        fn overlay(from: &Path, into: &Path, stop: Option<&AtomicBool>) -> io::Result<usize> {
            let mut names = HashMap::<OsString, OsString>::new();
            for entry in fs::read_dir(into)? {
                let name = entry?.file_name();
                let slot = names
                    .entry(name.to_ascii_lowercase())
                    .or_insert_with(|| name.clone());
                if name < *slot {
                    *slot = name;
                }
            }
            let mut files = 0;
            for entry in fs::read_dir(from)? {
                check_stop(stop)?;
                let entry = entry?;
                let name = entry.file_name();
                let existing = names.get(&name.to_ascii_lowercase());
                let destination = into.join(existing.unwrap_or(&name));
                if entry.file_type()?.is_dir() {
                    if existing.is_none() {
                        fs::create_dir(&destination)?;
                    }
                    files += overlay(&entry.path(), &destination, stop)?;
                } else {
                    if existing.is_some() {
                        fs::remove_file(&destination)?;
                    }
                    rename_new(&entry.path(), &destination)?;
                    files += 1;
                }
            }
            Ok(files)
        }

        fn check_original(path: &Path, modified: std::time::SystemTime) -> io::Result<()> {
            let current = fs::symlink_metadata(path)?;
            if is_link(&current) || current.modified()? != modified {
                return Err(invalid(format!(
                    "{} changed while adding files; try again",
                    path.display()
                )));
            }
            Ok(())
        }

        impl PreparedModAppend {
            pub fn publish(self) -> io::Result<usize> {
                plain_directory(&self.target)?;
                check_original(&self.target, self.root_modified)?;
                if self.files == 0 {
                    return Ok(0);
                }
                if let Some(name) = &self.package_layer {
                    rename_new(self.staged.path(), &self.target.join(name))?;
                } else {
                    replace_directory(&self.staged, &self.target)?;
                }
                Ok(self.files)
            }
        }

        #[cfg(unix)]
        fn replace_directory(staged: &TempDir, target: &Path) -> io::Result<()> {
            use std::ffi::CString;
            use std::os::unix::ffi::OsStrExt;
            let from = CString::new(staged.path().as_os_str().as_bytes())?;
            let to = CString::new(target.as_os_str().as_bytes())?;
            // Exchange the live and prepared directories in one filesystem operation.
            // SAFETY: both arguments are NUL-terminated and live for the call.
            #[cfg(target_os = "macos")]
            let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_SWAP) };
            // SAFETY: both paths are valid C strings and both dirfds refer to cwd.
            #[cfg(any(target_os = "linux", target_os = "android"))]
            let result = unsafe {
                libc::syscall(
                    libc::SYS_renameat2,
                    libc::AT_FDCWD,
                    from.as_ptr(),
                    libc::AT_FDCWD,
                    to.as_ptr(),
                    libc::RENAME_EXCHANGE,
                )
            };
            if result == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        }

        #[cfg(windows)]
        fn replace_directory(staged: &TempDir, target: &Path) -> io::Result<()> {
            let mods = target
                .parent()
                .ok_or_else(|| invalid("missing Mods folder"))?;
            let backup = staging_directory(".vera20k-backup-").tempdir_in(mods)?;
            let saved = backup.path().join("original");
            rename_new(target, &saved)?;
            if let Err(error) = rename_new(staged.path(), target) {
                if let Err(restore) = rename_new(&saved, target) {
                    let kept = backup.keep();
                    return Err(io::Error::other(format!(
                        "Install failed: {error}. Restore {} from {}: {restore}",
                        target.display(),
                        kept.join("original").display()
                    )));
                }
                return Err(error);
            }
            Ok(())
        }

        #[cfg(unix)]
        fn rename_new(from: &Path, to: &Path) -> io::Result<()> {
            use std::ffi::CString;
            use std::os::unix::ffi::OsStrExt;

            let from = CString::new(from.as_os_str().as_bytes())?;
            let to = CString::new(to.as_os_str().as_bytes())?;
            // SAFETY: both arguments are NUL-terminated and live for the call.
            #[cfg(target_os = "macos")]
            let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
            // SAFETY: both arguments are NUL-terminated and live for the call.
            #[cfg(target_os = "linux")]
            let result = unsafe {
                libc::renameat2(
                    libc::AT_FDCWD,
                    from.as_ptr(),
                    libc::AT_FDCWD,
                    to.as_ptr(),
                    libc::RENAME_NOREPLACE,
                )
            };
            // Android: bionic has no exclusive rename. The folder sits in the
            // app's private storage, which no other program writes, and the
            // name was looked up under the import lock.
            // SAFETY: both arguments are NUL-terminated and live for the call.
            #[cfg(target_os = "android")]
            let result = unsafe { libc::rename(from.as_ptr(), to.as_ptr()) };
            if result == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        }

        #[cfg(windows)]
        fn rename_new(from: &Path, to: &Path) -> io::Result<()> {
            use std::os::windows::ffi::OsStrExt;

            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn MoveFileW(from: *const u16, to: *const u16) -> i32;
            }

            let from: Vec<_> = from.as_os_str().encode_wide().chain([0]).collect();
            let to: Vec<_> = to.as_os_str().encode_wide().chain([0]).collect();
            // SAFETY: both paths are NUL-terminated and live for the call.
            if unsafe { MoveFileW(from.as_ptr(), to.as_ptr()) } != 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        }
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

    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    #[test]
    fn add_mod_applies_the_preview_order_for_zips_and_extracted_folders() {
        let downloads = tempfile::tempdir().unwrap();
        let mods = tempfile::tempdir().unwrap();
        let base = [("Rules.INI", b"base".as_slice()), ("Art.INI", b"art")];
        let patch = [("rules.ini", b"patched".as_slice())];
        for (part, files) in [("a Base", base.as_slice()), ("Z Patch", patch.as_slice())] {
            write_files(&downloads.path().join(part), files);
            write_zip(
                &downloads.path().join(format!("{part}.zip")),
                files,
                zip::CompressionMethod::Deflated,
            );
        }

        for (extension, installed) in [(".zip", "a Base"), ("", "a Base 2")] {
            let base_path = downloads.path().join(format!("a Base{extension}"));
            let patch_path = downloads.path().join(format!("Z Patch{extension}"));
            let plan = ModImport::plan(&[patch_path.clone(), base_path.clone()]).unwrap();
            assert_eq!(plan.parts().collect::<Vec<_>>(), [&base_path, &patch_path]);
            let folder = plan
                .prepare(mods.path(), &AtomicBool::new(false))
                .unwrap()
                .install()
                .unwrap();
            assert_eq!(folder, installed);
            let active = ModFolder::open(&mods.path().join(folder), None).unwrap();
            assert!(!active.is_prepared());
            assert_eq!(fs::read(active.join("Rules.INI")).unwrap(), b"patched");
            assert_eq!(fs::read(active.join("Art.INI")).unwrap(), b"art");
            assert!(base_path.exists() && patch_path.exists());
        }
        assert_eq!(
            fs::read(downloads.path().join("a Base/Rules.INI")).unwrap(),
            b"base"
        );
    }

    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "macos",
        target_os = "android"
    ))]
    #[test]
    fn append_preserves_the_installed_mod_and_its_identity() {
        let root = tempfile::tempdir().unwrap();
        let mods = root.path().join("Mods");
        let incoming = root.path().join("New");
        let target = mods.join("Selected");
        write_files(
            &target,
            &[
                ("maps01.mix", b"old"),
                ("mod.ini", b"identity"),
                (".vera20k-mod-id", b"id"),
            ],
        );
        write_files(
            &incoming,
            &[
                ("MAPS01.MIX", b"new"),
                ("MOD.INI", b"other"),
                (".vera20k-mod-id", b"other"),
            ],
        );
        let prepare = || {
            ModImport::plan(std::slice::from_ref(&incoming))
                .unwrap()
                .prepare_append(&mods, "Selected", &AtomicBool::new(false))
        };

        drop(prepare().unwrap());
        assert_eq!(fs::read(target.join("maps01.mix")).unwrap(), b"old");
        assert_eq!(prepare().unwrap().publish().unwrap(), 1);
        assert_eq!(fs::read(target.join("maps01.mix")).unwrap(), b"new");
        assert_eq!(fs::read(target.join("mod.ini")).unwrap(), b"identity");
        assert_eq!(fs::read(target.join(".vera20k-mod-id")).unwrap(), b"id");
    }

    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "macos",
        target_os = "android"
    ))]
    #[test]
    fn appended_part_loads_after_the_original_package() {
        let root = tempfile::tempdir().unwrap();
        let mods = root.path().join("Mods");
        let target = mods.join("Selected");
        fs::create_dir_all(&target).unwrap();
        write_zip(
            &target.join("base.zip"),
            &[
                ("base.mix", b"base"),
                ("art.ini", b"old"),
                ("mod.ini", b"identity"),
            ],
            zip::CompressionMethod::Stored,
        );
        let incoming = root.path().join("New");
        write_files(&incoming, &[("ART.INI", b"new"), ("MOD.INI", b"other")]);
        let staged = ModImport::plan(&[incoming])
            .unwrap()
            .prepare_append(&mods, "Selected", &AtomicBool::new(false))
            .unwrap();
        assert_eq!(staged.publish().unwrap(), 1);
        let opened = ModFolder::open(&target, None).unwrap();
        assert!(target.join("base.zip").is_file());
        assert_eq!(fs::read(opened.join("base.mix")).unwrap(), b"base");
        assert_eq!(fs::read(opened.join("art.ini")).unwrap(), b"new");
        assert_eq!(fs::read(opened.join("mod.ini")).unwrap(), b"identity");
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "android"))]
    #[test]
    fn append_refuses_a_link_in_the_installed_mod() {
        let root = tempfile::tempdir().unwrap();
        let mods = root.path().join("Mods");
        let target = mods.join("Selected");
        let outside = root.path().join("outside");
        let incoming = root.path().join("New");
        write_files(&target, &[("base.mix", b"base")]);
        write_files(&outside, &[("first.map", b"original")]);
        std::os::unix::fs::symlink(&outside, target.join("maps")).unwrap();
        write_files(&incoming, &[("maps/first.map", b"incoming")]);
        let result = ModImport::plan(&[incoming]).unwrap().prepare_append(
            &mods,
            "Selected",
            &AtomicBool::new(false),
        );
        assert!(result.is_err());
        assert_eq!(fs::read(outside.join("first.map")).unwrap(), b"original");
    }

    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    #[test]
    fn add_mod_keeps_a_chosen_order_when_more_parts_are_picked() {
        let downloads = tempfile::tempdir().unwrap();
        let mods = tempfile::tempdir().unwrap();
        let base = downloads.path().join("Z Base");
        let patch = downloads.path().join("b Patch.zip");
        let extra = downloads.path().join("a Extra");
        let conflict = downloads.path().join("other/z base");
        write_files(&base, &[("rules.ini", b"base")]);
        write_zip(
            &patch,
            &[("rules.ini", b"patched")],
            zip::CompressionMethod::Stored,
        );
        write_files(&extra, &[("art.ini", b"extra")]);
        write_files(&conflict, &[("rules.ini", b"wrong")]);

        let mut plan = ModImport::plan(std::slice::from_ref(&base)).unwrap();
        assert!(plan.add_paths(std::slice::from_ref(&patch)).unwrap());
        assert_eq!(plan.parts().collect::<Vec<_>>(), [&patch, &base]);
        assert!(plan.move_part(1, 0));
        assert!(!plan.move_part(0, 2));
        assert!(!plan.add_paths(std::slice::from_ref(&base)).unwrap());
        assert!(plan.add_paths(&[extra.clone(), conflict]).is_err());
        assert!(plan.add_paths(&[extra]).unwrap());

        let folder = plan
            .prepare(mods.path(), &AtomicBool::new(false))
            .unwrap()
            .install()
            .unwrap();
        let installed = mods.path().join(&folder);
        assert_eq!(fs::read(installed.join("rules.ini")).unwrap(), b"patched");
        assert_eq!(fs::read(installed.join("art.ini")).unwrap(), b"extra");
        assert_eq!(folder, "Z Base");
    }

    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    #[test]
    fn add_mod_discards_staged_files_when_a_later_part_cannot_be_read() {
        let downloads = tempfile::tempdir().unwrap();
        let mods = tempfile::tempdir().unwrap();
        let base = downloads.path().join("Base");
        let patch = downloads.path().join("Patch.zip");
        write_files(&base, &[("rules.ini", b"base")]);
        fs::write(&patch, b"not a zip").unwrap();

        let plan = ModImport::plan(&[base.clone(), patch]).unwrap();
        assert!(plan.prepare(mods.path(), &AtomicBool::new(false)).is_err());
        assert_eq!(fs::read_dir(mods.path()).unwrap().count(), 0);
        assert_eq!(fs::read(base.join("rules.ini")).unwrap(), b"base");
    }

    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    #[test]
    fn cancelling_add_mod_discards_a_finished_import_before_it_is_published() {
        let downloads = tempfile::tempdir().unwrap();
        let mods = tempfile::tempdir().unwrap();
        let source = downloads.path().join("Example");
        write_files(&source, &[("rules.ini", b"new")]);
        write_files(&mods.path().join("Example"), &[("rules.ini", b"installed")]);
        let plan = ModImport::plan(&[source]).unwrap();
        let staged = plan.prepare(mods.path(), &AtomicBool::new(false)).unwrap();
        drop(staged);
        assert_eq!(fs::read_dir(mods.path()).unwrap().count(), 1);
        assert_eq!(
            fs::read(mods.path().join("Example/rules.ini")).unwrap(),
            b"installed"
        );
    }
}
