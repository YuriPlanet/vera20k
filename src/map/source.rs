//! Resolve map names and paths through loose files and the retained asset VFS.
//!
//! Owns source selection and the identity of the exact bytes consumed by the
//! parser. Frontend discovery and tools use the same resolution path.

// Outside data reaches this module: it refuses bad input, never panics.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::panic))]

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::assets::asset_manager::{AssetManager, ResolvedAssetFile};
use crate::assets::error::AssetError;
use crate::map::map_file::{self, MapFile};
use crate::rules::ini_parser::IniFile;
use crate::util::sha256::sha256_hex;

const MAP_EXTENSIONS: [&str; 5] = ["mmx", "yro", "map", "mpr", "yrm"];
pub(super) const CUSTOM_MAP_EXTENSIONS: &[&str] = &["map", "yrm", "mpr"];
pub const MAX_TRANSFER_MAP_BYTES: usize = 8 * 1024 * 1024;
mod cache;
mod saved_path;
pub use cache::CacheLease;

/// Exact source whose bytes were parsed into the active map.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoadedMapSource {
    Loose {
        #[serde(flatten, with = "saved_path")]
        path: PathBuf,
        payload_len: usize,
        /// Hash of the consumed file buffer, including the wrapper for .mmx.
        source_sha256: String,
        /// Retain an installed download until the last active consumer drops it.
        #[serde(skip)]
        cache_lease: CacheLease,
    },
    Mix {
        logical_name: String,
        source_archive: String,
        entry_id: i32,
        payload_len: usize,
        /// Hash of the extracted entry bytes passed to the map INI parser.
        source_sha256: String,
    },
    Generated {
        seed_name: String,
        /// Actual seed-file input to generation. Older or missing-file sources
        /// retain `None`; their constructor-default gameplay remains valid.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seed_source: Option<GeneratedSeedSource>,
    },
    LegacyFallback {
        label: String,
    },
}

impl LoadedMapSource {
    /// Lookup name and external mission directory after restoring a saved world.
    /// A moved install can find its own copy by basename. Its old parent no
    /// longer supplies the directory for the next mission.
    ///
    /// VERA: a saved game is a file from a stranger, so its path is asked about
    /// only inside a map folder ([`saved_map_folder`]). Any other path, such as
    /// a network share or a device, is never touched and counts as moved. Under
    /// `launch_dir` the next mission is looked for there, as the launch did.
    pub fn restored_loose_lookup<'a>(
        &'a self,
        ra2_dir: Option<&'a Path>,
        launch_dir: Option<&'a Path>,
    ) -> (Option<&'a str>, Option<&'a Path>) {
        let Self::Loose { path, .. } = self else {
            return (None, None);
        };
        let folder = saved_map_folder(ra2_dir, launch_dir, path);
        if folder.is_some() && crate::util::file_system::is_file(path) {
            return (
                path.to_str().or_else(|| saved_path::basename(path)),
                if folder == launch_dir {
                    launch_dir
                } else {
                    path.parent().filter(|dir| !dir.as_os_str().is_empty())
                },
            );
        }
        (saved_path::basename(path), None)
    }

    pub fn physical_identity(&self) -> Option<(usize, &str)> {
        match self {
            Self::Loose {
                payload_len,
                source_sha256,
                ..
            }
            | Self::Mix {
                payload_len,
                source_sha256,
                ..
            } => Some((*payload_len, source_sha256)),
            Self::Generated {
                seed_source: Some(seed),
                ..
            } => Some((seed.payload_len, &seed.source_sha256)),
            _ => None,
        }
    }
}

/// VERA transfer identity is the exact consumed file, not the native [Digest]
/// text (which can be absent or shared by different files).
pub fn valid_source_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// The scenario's name for a map a launch resolved to a rooted path beside its
/// launch file (`-SPAWN`). That path names a temporary directory, and the name
/// is hashed state: recording and playback must agree wherever the map lives.
/// `has_root`, since `is_absolute` rejects the browser's `/spawn/...` paths.
pub fn portable_map_name(name: &str) -> &str {
    let path = Path::new(name);
    if path.has_root() {
        path.file_name()
            .and_then(|file| file.to_str())
            .unwrap_or(name)
    } else {
        name
    }
}

/// A logical file name: one that stays inside the folder it is joined to and
/// names a file there. No drive, root, `.` or `..` part and no Windows device,
/// under both separator spellings whatever OS runs the game, so a name from a
/// peer, a map, a mod or a saved game is admitted alike everywhere and can
/// never name a host path, a network share or a device.
pub fn is_logical_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 1024
        && !name.chars().any(char::is_control)
        && !name.contains(':')
        && name.split(['/', '\\']).all(|part| {
            !part.is_empty() && part != "." && part != ".." && !is_windows_device_name(part)
        })
}

/// Whether Windows reads `part` as a device in whatever folder it is, as it
/// does up to Windows 10: `CON`, `CONIN$`, `CONOUT$`, `PRN`, `AUX`, `NUL`, `COM0`-`COM9` and
/// `LPT0`-`LPT9` (also with a superscript 1-3), with any extension. Opening
/// such a name reads or writes the device, not a file
/// (Microsoft, "Naming Files, Paths, and Namespaces").
fn is_windows_device_name(part: &str) -> bool {
    let device = part
        .split('.')
        .next()
        .unwrap_or(part)
        .trim_end()
        .to_ascii_uppercase();
    let mut rest = device.chars().skip(3);
    matches!(
        device.as_str(),
        "CON" | "CONIN$" | "CONOUT$" | "PRN" | "AUX" | "NUL"
    ) || ((device.starts_with("COM") || device.starts_with("LPT"))
        && matches!(
            rest.next(),
            Some('0'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}')
        )
        && rest.next().is_none())
}

/// A multiplayer map name is a VFS-relative logical identity, never a peer's
/// host path, with the same admission on every client. Absolute cache paths
/// remain internal `LoadedMapSource` values and do not pass this boundary.
pub fn validate_network_map_name(name: &str) -> Result<()> {
    anyhow::ensure!(
        is_logical_name(name),
        "invalid logical multiplayer map name"
    );
    Ok(())
}

/// The map folder that holds a loose map path a saved game recorded, or `None`
/// when the path may not be opened again. A save is a file from a stranger, so
/// its path counts only inside a folder this game already reads maps from:
/// `launch_dir`, the folder an external launch named, or [`local_map_path`]'s.
/// Any other path, such as a network share or a device, is never touched: the
/// test reads the path's text only, because asking the file system about a
/// share already contacts it.
fn saved_map_folder<'a>(
    ra2_dir: Option<&'a Path>,
    launch_dir: Option<&'a Path>,
    path: &Path,
) -> Option<&'a Path> {
    let roots = launch_dir
        .into_iter()
        .chain(mod_dir())
        .chain(ra2_dir)
        .chain(downloaded_maps_dir());
    saved_map_folder_in(roots, path)
}

/// A folder counts only with a name of its own: an unset one, or a drive or
/// file-system root, would hold every path.
fn saved_map_folder_in<'a>(
    roots: impl IntoIterator<Item = &'a Path>,
    path: &Path,
) -> Option<&'a Path> {
    use std::path::Component;
    if path.components().any(|part| part == Component::ParentDir) {
        return None;
    }
    roots.into_iter().find(|root| {
        root.components()
            .any(|part| matches!(part, Component::Normal(_)))
            && path != *root
            && path.starts_with(root)
    })
}

/// Identity of the seed bytes consumed by the normal generated-map loader.
/// These bytes describe generation inputs, not the emitted map's INI bytes.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GeneratedSeedSource {
    pub path: PathBuf,
    pub payload_len: usize,
    pub source_sha256: String,
}

impl GeneratedSeedSource {
    pub fn from_consumed_bytes(path: PathBuf, bytes: &[u8]) -> Self {
        Self {
            path,
            payload_len: bytes.len(),
            source_sha256: sha256_hex(bytes),
        }
    }
}

/// Parsed map paired with the exact source consumed by the parser.
pub struct LoadedMap {
    pub map: MapFile,
    pub source: LoadedMapSource,
}

pub fn read_map_ini_for_metadata(path: &Path) -> Option<IniFile> {
    map_file::ini_from_file_bytes(crate::util::file_system::read(path).ok()?.into()).ok()
}

/// The selected loose map's INI with gameplay's directive resolution. Terrain
/// and other packed sections stay encoded while the chooser reads its header.
pub(super) fn read_effective_loose_map_ini(path: &Path, assets: &AssetManager) -> Option<IniFile> {
    let ini = read_map_ini_for_metadata(path)?;
    expand_map_ini_with_options(ini, &path.to_string_lossy(), assets, false).ok()
}

/// Physical INI plus the exact source bytes used by shell map verification.
/// Unlike full MapFile parsing, native6951F0 admits an INI without requiring
/// a playable map header before comparing its [Digest] text.
pub struct LoadedMapIni {
    pub ini: IniFile,
    pub source: LoadedMapSource,
}

struct MapInput<'a> {
    bytes: Cow<'a, [u8]>,
    source: LoadedMapSource,
}

impl<'a> MapInput<'a> {
    fn loose(path: &Path, bytes: Cow<'a, [u8]>) -> Self {
        let source = LoadedMapSource::Loose {
            cache_lease: Default::default(),
            path: path.to_path_buf(),
            payload_len: bytes.len(),
            source_sha256: sha256_hex(&bytes),
        };
        Self { bytes, source }
    }

    fn from_asset(logical_name: String, file: ResolvedAssetFile<'a>) -> Self {
        match file {
            ResolvedAssetFile::Loose { path, entry } => {
                Self::loose(path, Cow::Borrowed(entry.bytes))
            }
            ResolvedAssetFile::Mix(entry) => Self {
                bytes: Cow::Borrowed(entry.bytes),
                source: LoadedMapSource::Mix {
                    logical_name,
                    source_archive: entry.source_archive.to_owned(),
                    entry_id: entry.entry_id,
                    payload_len: entry.bytes.len(),
                    source_sha256: sha256_hex(entry.bytes),
                },
            },
        }
    }
}

static DOWNLOADED_MAPS_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// VERA's folder for maps it downloads or receives (`PathsConfig::
/// downloaded_maps_dir`), set once at start. Map lookups and Choose Map's
/// scan read it after the game folder; without it only the game folder counts.
/// It also holds the random-map dialog's files ([`random_map_dir`]).
pub fn set_downloaded_maps_dir(dir: PathBuf) {
    let _ = DOWNLOADED_MAPS_DIR.set(dir);
}

pub fn downloaded_maps_dir() -> Option<&'static Path> {
    DOWNLOADED_MAPS_DIR.get().map(PathBuf::as_path)
}

static MOD_DIR: std::sync::RwLock<Option<&'static Path>> = std::sync::RwLock::new(None);

/// VERA: the mod folder in use (`app::mods`), set at each start. Its maps
/// read as if they had been copied on top of the game folder: a lookup finds
/// a mod's file first, and Choose Map's scan lists it in place of a
/// game-folder file of the same name. A desktop run starts once; the browser
/// build starts again in one page, perhaps with another mod or none, so each
/// start sets it anew. The path is leaked to hand out `&'static`: a few bytes
/// a start.
pub fn set_mod_dir(dir: Option<PathBuf>) {
    let dir = dir.map(|dir| &*Box::leak(dir.into_boxed_path()));
    *MOD_DIR
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = dir;
}

pub fn mod_dir() -> Option<&'static Path> {
    *MOD_DIR
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The folders a listing of the game folder reads, for
/// `case_insensitive_path::native_listing_over`: `ra2_dir`, then the mod's.
pub fn game_folders(ra2_dir: &Path) -> Vec<&Path> {
    game_folders_in(ra2_dir, mod_dir())
}

pub(crate) fn game_folders_in<'a>(
    ra2_dir: &'a Path,
    selected_mod: Option<&'a Path>,
) -> Vec<&'a Path> {
    std::iter::once(ra2_dir).chain(selected_mod).collect()
}

/// The file `name` of the game folder: the mod's when it has one.
pub fn game_folder_file(ra2_dir: &Path, name: &str) -> PathBuf {
    game_folder_file_in(ra2_dir, mod_dir(), name)
}

pub(crate) fn game_folder_file_in(
    ra2_dir: &Path,
    selected_mod: Option<&Path>,
    name: &str,
) -> PathBuf {
    selected_mod
        .into_iter()
        .chain(std::iter::once(ra2_dir))
        .find_map(|root| existing_map_file(root.join(name)))
        .unwrap_or_else(|| ra2_dir.join(name))
}

/// Preserve an explicit spelling, then use the same case-insensitive basename
/// lookup as the asset provider before trying a lower-priority root.
fn existing_map_file(path: PathBuf) -> Option<PathBuf> {
    if crate::util::file_system::exists(&path) {
        return Some(path);
    }
    crate::util::case_insensitive_path::find(path.parent()?, path.file_name()?.to_str()?)
}

/// Where the random-map dialog's files live: `RandMap.Sed`, its `RandMap.img`
/// preview and the Random Maps subfolder for saved seeds. gamemd keeps them in its own folder
/// (`RandomMapSetupDialog__Run` 0x00595BC0 writes `RandMap.img`, its accepted
/// caller 0x005E8590 `RandMap.Sed`); VERA never writes the game folder, so
/// they are in its Maps folder. Without that folder (tests and tools), `root`.
pub fn random_map_dir(root: &Path) -> &Path {
    downloaded_maps_dir().unwrap_or(root)
}

/// A map file by name: the mod folder in use, then the game folder, then
/// VERA's downloaded maps. A seed is only looked for in [`random_map_dir`].
pub fn local_map_path(ra2_dir: &Path, map_name: &str) -> Result<PathBuf> {
    local_map_path_in(mod_dir(), ra2_dir, downloaded_maps_dir(), map_name)
}

/// Locate an installed authored scenario by its recorded bytes. A mod can
/// shadow a name without shadowing the manifest that supplied its recording.
pub fn matching_loose_map_path(
    ra2_dir: &Path,
    selected_mod: Option<&Path>,
    logical_name: &str,
    expected_sha256: &str,
) -> Option<PathBuf> {
    if !is_logical_name(logical_name) || !valid_source_sha256(expected_sha256) {
        return None;
    }
    game_folders_in(ra2_dir, selected_mod)
        .into_iter()
        .rev()
        .filter_map(|root| existing_map_file(root.join(logical_name)))
        .find(|path| {
            read_loose_map_input_bounded(path, Some(MAX_TRANSFER_MAP_BYTES))
                .ok()
                .and_then(|input| {
                    input
                        .source
                        .physical_identity()
                        .map(|(_, digest)| digest == expected_sha256)
                })
                .unwrap_or(false)
        })
}

fn local_map_path_in(
    mod_dir: Option<&Path>,
    ra2_dir: &Path,
    downloaded: Option<&Path>,
    map_name: &str,
) -> Result<PathBuf> {
    let direct = PathBuf::from(map_name);
    if direct.is_absolute() && crate::util::file_system::exists(&direct) {
        return Ok(direct);
    }
    // An older `RandMap.Sed` in the game folder is not the dialog's.
    let seed = crate::map::rmg::is_seed_selection(map_name);
    let first = if seed {
        downloaded.unwrap_or(ra2_dir)
    } else {
        ra2_dir
    };
    let roots = mod_dir
        .filter(|_| !seed)
        .into_iter()
        .chain(std::iter::once(first))
        .chain(downloaded.filter(|_| !seed));
    for root in roots {
        if let Some(exact) = existing_map_file(root.join(map_name)) {
            return Ok(exact);
        }
        for ext in MAP_EXTENSIONS {
            if let Some(candidate) = existing_map_file(root.join(format!("{map_name}.{ext}"))) {
                return Ok(candidate);
            }
        }
    }
    Err(anyhow::anyhow!(
        "Map '{}' not found (checked the retail root and .mmx/.yro/.map/.mpr/.yrm variants)",
        map_name
    ))
}

fn read_loose_map_input(path: &Path) -> Result<MapInput<'static>> {
    read_loose_map_input_bounded(path, None)
}

fn read_loose_map_input_bounded(path: &Path, limit: Option<usize>) -> Result<MapInput<'static>> {
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    let bytes = {
        use std::io::Read;
        let mut bytes = Vec::new();
        let file = std::fs::File::open(path)?;
        file.take(limit.map_or(u64::MAX, |limit| limit as u64 + 1))
            .read_to_end(&mut bytes)?;
        bytes
    };
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    let bytes = {
        let shared = crate::util::file_system::read_shared(path)?;
        anyhow::ensure!(
            limit.is_none_or(|limit| shared.len() <= limit),
            "map exceeds transfer size limit"
        );
        shared.to_vec()
    };
    anyhow::ensure!(
        limit.is_none_or(|limit| bytes.len() <= limit),
        "map exceeds transfer size limit"
    );
    Ok(MapInput::loose(path, Cow::Owned(bytes)))
}

fn decode_map(input: MapInput<'_>) -> Result<LoadedMap> {
    let map = if matches!(input.source, LoadedMapSource::Loose { .. }) {
        map_file::load_from_bytes(input.bytes.into_owned())?
    } else {
        MapFile::from_bytes(&input.bytes)?
    };
    Ok(LoadedMap {
        map,
        source: input.source,
    })
}

fn decode_metadata(input: MapInput<'_>) -> Result<LoadedMapIni> {
    Ok(LoadedMapIni {
        //695349 ignores INI.Load's boolean before ReadString. A malformed
        // physical file still supplies the reader default; it must not cause
        // a fallback to a different MIX payload.
        ini: map_file::ini_from_file_bytes(input.bytes).unwrap_or_else(|_| IniFile::empty()),
        source: input.source,
    })
}

fn expand_map_ini(
    ini: IniFile,
    source: &LoadedMapSource,
    assets: &AssetManager,
) -> Result<IniFile> {
    let name = match source {
        LoadedMapSource::Loose { path, .. } => path.to_string_lossy(),
        LoadedMapSource::Mix { logical_name, .. } => Cow::Borrowed(logical_name.as_str()),
        _ => Cow::Borrowed("<map>"),
    };
    expand_map_ini_with_options(ini, &name, assets, false)
}

fn expand_map_ini_with_options(
    ini: IniFile,
    name: &str,
    assets: &AssetManager,
    strict: bool,
) -> Result<IniFile> {
    if !(crate::rules::ares_switch::ares_way() || crate::rules::PHOBOS_ENABLED) {
        return Ok(ini);
    }
    let (expanded, _) = crate::rules::retail_sources::expand_ini_with_assets(
        assets,
        ini,
        name,
        crate::rules::ini_parser::directives::DirectiveOptions {
            strict,
            ..crate::rules::ini_parser::directives::DirectiveOptions::production()
        },
    )
    .map_err(anyhow::Error::msg)?;
    Ok(expanded.ini)
}

/// Network admission uses the exact ordinary expansion owner, but a missing or
/// cyclic dependency cannot silently become a different multiplayer map.
pub fn resolve_network_map_ini(
    mut loaded: LoadedMapIni,
    assets: &AssetManager,
    seed: bool,
    logical_name: &str,
) -> Result<LoadedMapIni> {
    if !seed {
        loaded.ini = expand_map_ini_with_options(loaded.ini, logical_name, assets, true)?;
    }
    Ok(loaded)
}

/// Run the ordinary map parser only after its shared include expansion. A
/// downloaded include root can legitimately obtain its Map/pack sections from
/// the local dependency closure, but that closure must make a playable map.
pub fn validate_network_map_content(ini: &IniFile, seed: bool) -> Result<()> {
    if !seed {
        MapFile::from_ini(ini.clone())?;
    }
    Ok(())
}

/// `ares`: Ares.dll is loaded (the game is read the Ares way); only a seed
/// reads it, whose bytes then carry the Urban Areas box
/// (`RmgOptions::to_sed_bytes`).
pub fn effective_network_map_digest(ini: &IniFile, seed: bool, ares: bool) -> String {
    if seed {
        let mut options = crate::map::rmg::RmgOptions::default();
        options.apply_sed(ini, ares);
        options.normalize();
        return IniFile::from_bytes(&options.to_sed_bytes(ares))
            .expect("serialized seed INI")
            .multiplayer_digest();
    }
    let mut checked = ini.clone();
    checked.remove_digest_for_checked_load();
    checked.multiplayer_digest()
}

pub fn load_network_map_ini(
    root: &Path,
    name: &str,
    assets: &AssetManager,
) -> Result<LoadedMapIni> {
    // Select the physical winner before checking its dependency closure. An
    // absent include must not turn a loose winner into a different MIX map.
    let loaded = load_map_input_with_assets(
        root,
        name,
        assets,
        Some(MAX_TRANSFER_MAP_BYTES),
        decode_metadata,
    )?;
    resolve_network_map_ini(
        loaded,
        assets,
        crate::map::rmg::is_seed_selection(name),
        name,
    )
}

fn decode_map_with_assets(input: MapInput<'_>, assets: &AssetManager) -> Result<LoadedMap> {
    let ini = if matches!(input.source, LoadedMapSource::Loose { .. }) {
        map_file::ini_from_file_bytes(input.bytes)?
    } else {
        IniFile::from_bytes(&input.bytes)?
    };
    let ini = expand_map_ini(ini, &input.source, assets)?;
    Ok(LoadedMap {
        map: MapFile::from_ini(ini)?,
        source: input.source,
    })
}

fn decode_metadata_with_assets(input: MapInput<'_>, assets: &AssetManager) -> Result<LoadedMapIni> {
    let mut loaded = decode_metadata(input)?;
    loaded.ini = expand_map_ini(loaded.ini, &loaded.source, assets)?;
    Ok(loaded)
}

/// Both gameplay and shell metadata use this one loose-before-MIX resolution
/// policy and the supplied provider's mod choice, not another process-global
/// selection. Preserve the existing fallback on a failed loose parse.
fn load_map_input_with_assets<T>(
    ra2_dir: &Path,
    map_name: &str,
    assets: &AssetManager,
    limit: Option<usize>,
    decode: impl Fn(MapInput<'_>) -> Result<T>,
) -> Result<T> {
    let local = local_map_path_in(assets.mod_dir(), ra2_dir, downloaded_maps_dir(), map_name)
        .and_then(|path| read_loose_map_input_bounded(&path, limit))
        .and_then(&decode);
    match local {
        Ok(value) => Ok(value),
        Err(local_err) => {
            for candidate in asset_map_candidates(map_name) {
                let file = assets
                    .resolve_file_with_limit(&candidate, limit)
                    .map_err(|error| match error {
                        AssetError::ReadLimitExceeded { .. } => {
                            anyhow::anyhow!("map exceeds transfer size limit")
                        }
                        error => error.into(),
                    })?;
                if let Some(file) = file {
                    return decode(MapInput::from_asset(candidate, file));
                }
            }
            Err(local_err)
        }
    }
}

pub fn load_map_by_name_or_path_with_assets(
    ra2_dir: &Path,
    map_name: &str,
    assets: &AssetManager,
) -> Result<LoadedMap> {
    load_map_input_with_assets(ra2_dir, map_name, assets, None, |input| {
        decode_map_with_assets(input, assets)
    })
}

/// The file a game of `map_name` would load, chosen as metadata and gameplay
/// choose it (loose before MIX) but not parsed: its source, whose
/// `physical_identity` is the file's length and SHA-256.
pub fn map_source_identity(
    ra2_dir: &Path,
    map_name: &str,
    assets: &AssetManager,
) -> Result<LoadedMapSource> {
    load_map_input_with_assets(ra2_dir, map_name, assets, None, |input| Ok(input.source))
}

pub fn load_map_ini_by_name_or_path_with_assets(
    ra2_dir: &Path,
    map_name: &str,
    assets: &AssetManager,
) -> Result<LoadedMapIni> {
    load_map_input_with_assets(ra2_dir, map_name, assets, None, |input| {
        decode_metadata_with_assets(input, assets)
    })
}

/// Read upload bytes through the same loose-before-MIX source selection as
/// metadata and gameplay, with a hard allocation bound.
pub enum TransferMapSource {
    Loose {
        path: PathBuf,
        cache_lease: CacheLease,
    },
    Memory(Vec<u8>),
}

pub fn prepare_transfer_map_source(
    source: &LoadedMapSource,
    assets: &AssetManager,
) -> Result<TransferMapSource> {
    match source {
        LoadedMapSource::Loose {
            path, cache_lease, ..
        } => Ok(TransferMapSource::Loose {
            path: path.clone(),
            cache_lease: cache_lease.clone(),
        }),
        LoadedMapSource::Mix { logical_name, .. } => {
            let resolved = assets
                .resolve_ref(logical_name)
                .ok_or_else(|| anyhow::anyhow!("selected map source disappeared"))?;
            anyhow::ensure!(
                resolved.bytes.len() <= MAX_TRANSFER_MAP_BYTES,
                "map exceeds transfer size limit"
            );
            Ok(TransferMapSource::Memory(resolved.bytes.to_vec()))
        }
        _ => anyhow::bail!("selected map has no transferable file"),
    }
}

pub fn transfer_file_source(path: &Path, resource: TransferResource) -> Result<LoadedMapSource> {
    let input = read_loose_map_input_bounded(path, Some(resource.limit()))?;
    decode_transfer(&input.bytes, resource)?;
    Ok(input.source)
}

impl TransferMapSource {
    pub fn read(self, expected_digest: &str, expected_length: usize) -> Result<Vec<u8>> {
        let bytes = match self {
            Self::Loose { path, cache_lease } => {
                let input = read_loose_map_input_bounded(
                    &path,
                    Some(expected_length.min(MAX_TRANSFER_MAP_BYTES)),
                )?;
                drop(cache_lease);
                input.bytes.into_owned()
            }
            Self::Memory(bytes) => bytes,
        };
        anyhow::ensure!(
            bytes.len() == expected_length
                && bytes.len() <= MAX_TRANSFER_MAP_BYTES
                && sha256_hex(&bytes) == expected_digest,
            "map bytes changed before upload"
        );
        Ok(bytes)
    }
}

/// Which normal loader consumes the received physical bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferResource {
    Map,
    Seed,
    Preview,
}
impl TransferResource {
    pub fn limit(self) -> usize {
        if self == Self::Preview {
            512 * 1024
        } else {
            MAX_TRANSFER_MAP_BYTES
        }
    }
    fn extension(self) -> &'static str {
        match self {
            Self::Map => "map",
            Self::Seed => "sed",
            Self::Preview => "pcx",
        }
    }
}

pub enum TransferContent {
    Scenario(IniFile),
    Preview(crate::map::preview::DecodedPreview),
}
pub struct LoadedTransferAsset {
    pub source: LoadedMapSource,
    pub content: TransferContent,
}

/// Whether this build's gameplay selection will expand includes after the
/// transfer is installed, as the directive owner selects them.
fn include_expansion_will_run(ini: &IniFile) -> bool {
    crate::rules::ini_parser::directives::DirectiveOptions::production().expands_includes(ini)
}

fn decode_transfer(bytes: &[u8], resource: TransferResource) -> Result<TransferContent> {
    if resource == TransferResource::Preview {
        anyhow::ensure!(bytes.len() >= 128, "preview header missing");
        let width = u16::from_le_bytes([bytes[8], bytes[9]])
            .checked_sub(u16::from_le_bytes([bytes[4], bytes[5]]))
            .and_then(|n| n.checked_add(1))
            .ok_or_else(|| anyhow::anyhow!("invalid preview bounds"))?;
        let height = u16::from_le_bytes([bytes[10], bytes[11]])
            .checked_sub(u16::from_le_bytes([bytes[6], bytes[7]]))
            .and_then(|n| n.checked_add(1))
            .ok_or_else(|| anyhow::anyhow!("invalid preview bounds"))?;
        // Transferred PCX and packed-map previews share the safe edge limit;
        // the network resource retains its stricter total-pixel allowance.
        let pixels =
            crate::map::preview::expected_preview_pixels(u32::from(width), u32::from(height))?;
        anyhow::ensure!(pixels <= 2 * 1024 * 1024, "preview dimensions exceed limit");
        let scan = usize::from(u16::from_le_bytes([bytes[66], bytes[67]]))
            .checked_mul(usize::from(bytes[65]))
            .and_then(|n| n.checked_mul(usize::from(height)));
        anyhow::ensure!(
            scan.is_some_and(|n| n <= 8 * 1024 * 1024),
            "preview dimensions exceed limit"
        );
        let image = crate::assets::pcx_file::PcxFile::from_transfer_bytes(bytes)?;
        return Ok(TransferContent::Preview(
            crate::map::preview::DecodedPreview {
                width: u32::from(image.width),
                height: u32::from(image.height),
                rgba: image.to_rgba(None),
            },
        ));
    }
    let ini = map_file::ini_from_transfer_bytes(bytes.to_vec(), resource == TransferResource::Map)?;
    if resource == TransferResource::Seed {
        anyhow::ensure!(
            ini.section("RandomMap").is_some(),
            "random-map seed options missing"
        );
        let mut options = crate::map::rmg::RmgOptions::default();
        // The options are read and dropped: whether Ares.dll is loaded (the
        // Urban Areas box) changes nothing here.
        options.apply_sed(&ini, false);
        options.normalize();
    } else if !include_expansion_will_run(&ini) {
        map_file::MapFile::from_ini(ini.clone())?;
    }
    Ok(TransferContent::Scenario(ini))
}

/// Exact bytes are verified before any publication. Only locally generated
/// names are written; failed validation/writes leave no published partial file.
pub fn install_received_asset(
    directory: &Path,
    bytes: Vec<u8>,
    digest: &str,
    length: usize,
    resource: TransferResource,
) -> Result<LoadedTransferAsset> {
    install_received_asset_with_limits(
        directory,
        bytes,
        digest,
        length,
        resource,
        cache::Limits::default(),
    )
}

fn install_received_asset_with_limits(
    directory: &Path,
    bytes: Vec<u8>,
    digest: &str,
    length: usize,
    resource: TransferResource,
    limits: cache::Limits,
) -> Result<LoadedTransferAsset> {
    anyhow::ensure!(valid_source_sha256(digest), "invalid map source identity");
    anyhow::ensure!(
        length > 0 && length <= resource.limit() && bytes.len() == length,
        "invalid received map length"
    );
    anyhow::ensure!(
        sha256_hex(&bytes) == digest,
        "received map checksum differs"
    );
    let content = decode_transfer(&bytes, resource)?;
    let cache = cache::Cache::open(directory, true, limits)?;
    let path = cache.path(digest, resource)?;
    let already_cached = read_loose_map_input_bounded(&path, Some(resource.limit()))
        .is_ok_and(|previous| previous.bytes.as_ref() == bytes.as_slice());
    cache.make_room(&path, if already_cached { 0 } else { length })?;
    #[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
    if !already_cached {
        // Validation and cache reservation have completed. The browser VFS
        // publishes the complete byte buffer in one synchronous replacement,
        // then schedules the existing /saves persistence callback.
        crate::util::file_system::write(&path, &bytes)?;
    }
    #[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
    if !already_cached {
        crate::util::file_system::write_staged(cache.directory(), &bytes, |stage| {
            std::fs::rename(stage, &path)
        })?;
    }
    let source = LoadedMapSource::Loose {
        cache_lease: cache.pin(&path)?,
        path,
        payload_len: length,
        source_sha256: digest.to_owned(),
    };
    Ok(LoadedTransferAsset { source, content })
}

/// The name a received or downloaded map is kept under in VERA's Maps folder: the
/// last part of `host_file` with `.map` for its extension. Refused when that
/// would be hidden (a leading dot) or hold a character Windows forbids in file
/// names; a name Windows reserves is no logical name ([`is_logical_name`]). A
/// name that passes is one every platform can save.
pub fn kept_map_file_name(host_file: &str) -> Result<String> {
    validate_network_map_name(host_file)?;
    let stem = host_file
        .rsplit(['/', '\\'])
        .next()
        .and_then(|name| Path::new(name).file_stem())
        .and_then(|stem| stem.to_str())
        .filter(|stem| {
            !stem.is_empty()
                && !stem.starts_with('.')
                && !stem.ends_with([' ', '.'])
                && !stem.contains(['<', '>', '"', '|', '?', '*'])
        })
        .ok_or_else(|| anyhow::anyhow!("received map has no file name"))?;
    Ok(format!("{stem}.map"))
}

/// File types that can hold an ordinary custom map in VERA's Maps folder.
pub fn is_custom_map_name(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            CUSTOM_MAP_EXTENSIONS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

/// Whether `path` names a loose map file by its extension (`.yrm`, `.map`
/// and the rest a map lookup tries).
pub fn is_loose_map_name(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            MAP_EXTENSIONS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

/// Where [`keep_received_map`] keeps a map: its file and that file's name.
pub struct KeptMap {
    pub path: PathBuf,
    pub name: String,
}

/// The names a map is kept under, in the order [`keep_received_map`] tries
/// them: `kept_name`, which [`kept_map_file_name`] gave, then
/// `<stem>-<first 8 hex of its SHA-256>.map` for when another map holds the
/// first.
pub fn kept_map_names(kept_name: &str, sha256: &str) -> [String; 2] {
    let stem = kept_name.strip_suffix(".map").unwrap_or(kept_name);
    let prefix: String = sha256.chars().take(8).collect();
    [kept_name.to_owned(), format!("{stem}-{prefix}.map")]
}

/// Snapshot of the folders a loose map lookup searches, highest priority first.
/// Index by length so offering a whole online catalog only reads plausible matches.
#[derive(Default)]
pub struct KeptMapFiles {
    names: HashSet<String>,
    by_length: HashMap<u64, Vec<MapCopy>>,
}

struct MapCopy {
    path: PathBuf,
    digest: std::cell::OnceCell<Option<String>>,
}

impl KeptMapFiles {
    pub fn scan(folders: &[&Path]) -> Self {
        let mut files = Self::default();
        for folder in folders {
            let mut stems = Vec::new();
            for path in
                crate::util::case_insensitive_path::native_listing(folder).unwrap_or_default()
            {
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if is_loose_map_name(&path)
                    && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                {
                    stems.push(stem.to_lowercase());
                }
                if files.names.insert(name.to_lowercase())
                    && is_custom_map_name(&path)
                    && !name.contains('\\')
                    && kept_map_file_name(name).is_ok()
                    && let Ok(meta) = crate::util::file_system::metadata(&path)
                    && meta.is_file()
                {
                    files
                        .by_length
                        .entry(meta.len())
                        .or_default()
                        .push(MapCopy {
                            path,
                            digest: std::cell::OnceCell::new(),
                        });
                }
            }
            // local_map_path_in tries exact names before appended extensions in
            // each folder. A stem can therefore shadow a file in a lower folder.
            files.names.extend(stems);
        }
        files
    }

    pub fn contains_name(&self, name: &str) -> bool {
        self.names.contains(&name.to_lowercase())
    }

    /// Return the matching copy under its actual name and case.
    pub fn existing(&self, length: usize, sha256: &str) -> Option<KeptMap> {
        let copies = self.by_length.get(&(length as u64))?;
        copies.iter().find_map(|copy| {
            // Several catalog entries can share a length. Hash each candidate
            // only once in this scan, when one of those entries needs it.
            let digest = copy.digest.get_or_init(|| {
                let input = read_loose_map_input_bounded(&copy.path, Some(length)).ok()?;
                let (bytes, digest) = input.source.physical_identity()?;
                (bytes == length).then(|| digest.to_owned())
            });
            if digest.as_deref() != Some(sha256) {
                return None;
            }
            Some(KeptMap {
                path: copy.path.clone(),
                name: copy.path.file_name()?.to_str()?.to_owned(),
            })
        })
    }
}

/// Keeps a map downloaded or received in a room in VERA's `maps_dir`, never
/// the retail installation; Choose Map lists it from there, so the player can
/// host it later. `host_file` is the host's logical map name and `bytes` the
/// verified file. An existing copy with the same bytes is reused under
/// its original name. A new copy takes the first free name from [`kept_map_names`].
pub fn keep_received_map(
    maps_dir: &Path,
    ra2_dir: &Path,
    host_file: &str,
    bytes: &[u8],
) -> Result<KeptMap> {
    keep_received_map_with_names(maps_dir, ra2_dir, host_file, bytes, |_| false)
}

/// Also reserve names that are already selectable without a loose file, such as
/// a catalog entry in a MIX archive.
pub fn keep_received_map_with_names(
    maps_dir: &Path,
    ra2_dir: &Path,
    host_file: &str,
    bytes: &[u8],
    occupied: impl Fn(&str) -> bool,
) -> Result<KeptMap> {
    let name = kept_map_file_name(host_file)?;
    let digest = sha256_hex(bytes);
    let folders: Vec<_> = mod_dir().into_iter().chain([ra2_dir, maps_dir]).collect();
    // Serialize desktop writers across the lookup and save, even when two rooms
    // send identical bytes with different names. The browser VFS runs synchronously.
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    let lock = (|| -> Result<std::fs::File> {
        crate::util::file_system::create_dir_all(maps_dir)?;
        let file = cache::open_regular(&maps_dir.join(".keep-map.lock"), true, true)?;
        file.lock()?;
        Ok(file)
    })();
    let files = KeptMapFiles::scan(&folders);
    if let Some(kept) = files.existing(bytes.len(), &digest) {
        return Ok(kept);
    }
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    let _lock = lock?;
    for name in kept_map_names(&name, &digest) {
        if !files.contains_name(&name)
            && !occupied(&name)
            && write_kept_map(maps_dir, &name, bytes)?
        {
            return Ok(KeptMap {
                path: maps_dir.join(&name),
                name,
            });
        }
    }
    anyhow::bail!("other maps already hold the names {name} and its alternative")
}

/// Writes a new `name` in `maps_dir`; false when the file system says that
/// name is already taken by a spelling the directory scan did not match.
fn write_kept_map(maps_dir: &Path, name: &str, bytes: &[u8]) -> Result<bool> {
    crate::util::file_system::create_dir_all(maps_dir)?;
    match crate::util::file_system::write_new(maps_dir.join(name), bytes) {
        Ok(()) => Ok(true),
        #[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub fn load_received_asset(
    directory: &Path,
    digest: &str,
    length: usize,
    resource: TransferResource,
) -> Result<LoadedTransferAsset> {
    anyhow::ensure!(
        valid_source_sha256(digest) && length > 0 && length <= resource.limit(),
        "invalid cached map identity"
    );
    let cache = cache::Cache::open(directory, false, cache::Limits::default())?;
    let path = cache.path(digest, resource)?;
    let mut input = read_loose_map_input_bounded(&path, Some(resource.limit()))?;
    anyhow::ensure!(
        input.source.physical_identity() == Some((length, digest)),
        "cached map identity differs"
    );
    let content = decode_transfer(&input.bytes, resource)?;
    cache.make_room(&path, 0)?;
    let LoadedMapSource::Loose { cache_lease, .. } = &mut input.source else {
        unreachable!("loose cache input");
    };
    *cache_lease = cache.pin(&path)?;
    Ok(LoadedTransferAsset {
        source: input.source,
        content,
    })
}

pub fn asset_map_candidates(map_name: &str) -> Vec<String> {
    let mut names = Vec::new();
    names.push(map_name.to_string());
    let has_extension = Path::new(map_name).extension().is_some();
    if !has_extension {
        for ext in MAP_EXTENSIONS {
            names.push(format!("{map_name}.{ext}"));
        }
    }
    names
}

fn load_map_from_path_with_source(path: &Path) -> Result<LoadedMap> {
    decode_map(read_loose_map_input(path)?)
}

/// Try loading .mmx map files from a list of candidates.
pub fn try_load_mmx(ra2_dir: &Path, names: &[&str]) -> Result<LoadedMap> {
    for &name in names {
        let path: PathBuf = ra2_dir.join(name);
        if crate::util::file_system::exists(&path) {
            match load_map_from_path_with_source(&path) {
                Ok(loaded) => {
                    log::info!("Loaded map from {}", name);
                    return Ok(loaded);
                }
                Err(err) => {
                    log::warn!("Failed to load {}: {:#}", name, err);
                }
            }
        }
    }
    Err(anyhow::anyhow!(
        "No .mmx map files found in {}",
        ra2_dir.display()
    ))
}

#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use super::*;
    use crate::assets::mix_hash::mix_hash;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    pub struct TestDirectory(PathBuf);

    impl TestDirectory {
        pub fn new(label: &str) -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "vera20k-map-source-{label}-{}-{sequence}",
                std::process::id()
            ));
            std::fs::create_dir(&path).expect("create test directory");
            Self(path)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }

        pub fn write(&self, name: &str, bytes: &[u8]) {
            std::fs::write(self.0.join(name), bytes).expect("write test file");
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// [`install_received_asset`] for an authored map, with its INI.
    pub fn install_received_map(
        directory: &Path,
        bytes: Vec<u8>,
        digest: &str,
        length: usize,
    ) -> Result<LoadedMapIni> {
        let LoadedTransferAsset {
            source,
            content: TransferContent::Scenario(ini),
        } = install_received_asset(directory, bytes, digest, length, TransferResource::Map)?
        else {
            anyhow::bail!("an authored map installs as a scenario");
        };
        Ok(LoadedMapIni { source, ini })
    }

    pub fn make_new_format_mix_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let body_size: usize = entries.iter().map(|(_, body)| body.len()).sum();
        let mut data = Vec::new();
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(
            &u16::try_from(entries.len())
                .expect("test MIX entry count")
                .to_le_bytes(),
        );
        data.extend_from_slice(
            &u32::try_from(body_size)
                .expect("test MIX body size")
                .to_le_bytes(),
        );

        let mut offset = 0u32;
        for (name, body) in entries {
            data.extend_from_slice(&mix_hash(name).to_le_bytes());
            data.extend_from_slice(&offset.to_le_bytes());
            data.extend_from_slice(
                &u32::try_from(body.len())
                    .expect("test MIX entry size")
                    .to_le_bytes(),
            );
            offset += u32::try_from(body.len()).expect("test MIX entry offset");
        }
        for (_, body) in entries {
            data.extend_from_slice(body);
        }
        data
    }
}

#[cfg(all(test, tests_map))]
mod tests {
    use super::test_support::{TestDirectory, make_new_format_mix_bytes};

    #[test]
    fn replay_finds_recorded_bytes_behind_a_mods_same_named_map() {
        let retail = TestDirectory::new("retail-recorded-scenario");
        let selected_mod = TestDirectory::new("mod-same-named-scenario");
        retail.write("opening.map", b"recorded retail scenario");
        selected_mod.write("opening.map", b"conflicting mod scenario");
        let digest = crate::util::sha256::sha256_hex(b"recorded retail scenario");
        assert_eq!(
            super::matching_loose_map_path(
                retail.path(),
                Some(selected_mod.path()),
                "opening.map",
                &digest
            ),
            Some(retail.path().join("opening.map")),
        );
    }

    #[cfg(unix)]
    #[test]
    fn saved_loose_path_stays_readable_across_readers_and_platforms() {
        use super::LoadedMapSource;
        use std::os::unix::ffi::OsStrExt;
        use std::path::PathBuf;

        let loose = |path| LoadedMapSource::Loose {
            path,
            cache_lease: Default::default(),
            payload_len: 12,
            source_sha256: "saved-digest".into(),
        };
        let ordinary = loose("maps/Mission.map".into());
        let old = serde_json::to_value(&ordinary).unwrap();
        assert_eq!(old["path"], "maps/Mission.map");
        assert!(old.get("native_path").is_none());
        assert_eq!(
            serde_json::from_value::<LoadedMapSource>(old).unwrap(),
            ordinary
        );

        let path = std::ffi::OsStr::from_bytes(b"/game/\xff/Mission.map");
        let source = loose(path.into());
        let mut wire = serde_json::to_value(&source).unwrap();
        assert_eq!(
            wire["native_path"]["Unix"],
            serde_json::json!(path.as_bytes())
        );
        #[derive(serde::Deserialize)]
        struct OldLooseFields {
            path: PathBuf,
        }
        let old: OldLooseFields = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(old.path, PathBuf::from("Mission.map"));
        assert_eq!(
            serde_json::from_value::<LoadedMapSource>(wire.clone()).unwrap(),
            source
        );

        // A Windows save with a non-Unicode parent retains the same portable name.
        let windows: Vec<u16> = [0xd800, b'\\' as u16]
            .into_iter()
            .chain("Mission.map".encode_utf16())
            .collect();
        wire["native_path"] = serde_json::json!({ "Windows": windows });
        let foreign: LoadedMapSource = serde_json::from_value(wire).unwrap();
        assert_eq!(foreign, loose("Mission.map".into()));
    }

    #[test]
    fn a_received_map_is_kept_in_veras_folder_without_replacing_the_players_file() {
        let game = TestDirectory::new("keep-received-game");
        let vera = TestDirectory::new("keep-received-vera");
        let maps = vera.path().join("Maps");
        let map = b"[Basic]\nName=Frozen Lakes\nGameMode=standard\n";
        let kept = keep_received_map(&maps, game.path(), "Custom/FrozenLakes.map", map).unwrap();
        assert_eq!(kept.name, "FrozenLakes.map");
        assert!(
            !game.path().join("FrozenLakes.map").exists(),
            "nothing goes into the game folder"
        );
        // Found by name after the game folder, as every map lookup does.
        assert_eq!(
            local_map_path_in(None, game.path(), Some(&maps), "FrozenLakes.map").unwrap(),
            maps.join("FrozenLakes.map")
        );
        let record =
            crate::map::scenario_sources::loose_map_record(&kept.path, kept.name.clone(), 0, None)
                .expect("Choose Map reads the kept map");
        assert_eq!(record.display_name, "Frozen Lakes");
        // The same map again, in another case, is the file already kept.
        let again = keep_received_map(&maps, game.path(), "frozenlakes.YRM", map).unwrap();
        assert_eq!(again.path, maps.join("FrozenLakes.map"));
        // Another map holding the name, in either folder and in any case
        // (Unicode too), stays as it is; this one takes its SHA-256 suffix.
        let lakes = b"[Basic]\nName=Lakes\n";
        let suffix = &sha256_hex(lakes)[..8];
        game.write("lakes.MAP", b"[Basic]\nName=Mine\n");
        let kept = keep_received_map(&maps, game.path(), "Lakes.yrm", lakes).unwrap();
        assert_eq!(kept.name, format!("Lakes-{suffix}.map"));
        assert_eq!(
            std::fs::read(game.path().join("lakes.MAP")).unwrap(),
            b"[Basic]\nName=Mine\n"
        );
        let uber = b"[Basic]\nName=Uber\n";
        let suffix = &sha256_hex(uber)[..8];
        std::fs::write(maps.join("\u{fc}ber.map"), b"[Basic]\nName=Mine too\n").unwrap();
        let kept = keep_received_map(&maps, game.path(), "\u{dc}ber.map", uber).unwrap();
        assert_eq!(kept.name, format!("\u{dc}ber-{suffix}.map"));
        assert_eq!(
            std::fs::read(maps.join("\u{fc}ber.map")).unwrap(),
            b"[Basic]\nName=Mine too\n"
        );
        assert!(keep_received_map(&maps, game.path(), "../Escape.yrm", map).is_err());
        assert!(keep_received_map(&maps, game.path(), "What?.yrm", map).is_err());
    }

    #[test]
    fn exports_and_downloads_reuse_identical_legacy_files_before_creating_a_map() {
        let game = TestDirectory::new("old-map-game");
        let maps = TestDirectory::new("old-map-copies");
        let export = b"[Basic]\nName=Generated\n";
        let legacy = format!("rmap42-{}.YRM", &sha256_hex(export)[..8]);
        maps.write(&legacy, export);
        let other = b"[Basic]\nName=Manually renamed\n";
        maps.write("My favorite.MpR", other);
        let kept = keep_received_map(maps.path(), game.path(), "rmap42", export).unwrap();
        assert_eq!(kept.name, legacy);
        let kept =
            keep_received_map(maps.path(), game.path(), "Community/Arena.yrm", other).unwrap();
        assert_eq!(kept.name, "My favorite.MpR");
        assert!(!maps.path().join("Arena.map").exists());
        assert!(!maps.path().join("rmap42.map").exists());
    }

    #[test]
    fn simultaneous_downloads_under_different_names_keep_one_map() {
        let game = TestDirectory::new("simultaneous-map-game");
        let maps = TestDirectory::new("simultaneous-map-copies");
        let start = std::sync::Barrier::new(2);
        let save = |name| {
            start.wait();
            keep_received_map(maps.path(), game.path(), name, b"[Basic]\nName=Arena\n")
                .unwrap()
                .path
        };
        std::thread::scope(|scope| {
            let first = scope.spawn(|| save("Arena.map"));
            let second = save("Arena Edition.mpr");
            assert_eq!(first.join().unwrap(), second);
        });
    }

    #[test]
    fn a_random_map_seed_is_only_read_from_veras_folder() {
        let game = TestDirectory::new("seed-lookup-game");
        let vera = TestDirectory::new("seed-lookup-vera");
        // An older seed the original game left in its folder is never the
        // random-map dialog's: not even when VERA's folder has none yet.
        game.write("RandMap.Sed", b"[RandomMap]\nSeed=1\n");
        assert!(local_map_path_in(None, game.path(), Some(vera.path()), "RandMap.Sed").is_err());
        vera.write("RandMap.Sed", b"[RandomMap]\nSeed=2\n");
        assert_eq!(
            local_map_path_in(None, game.path(), Some(vera.path()), "RandMap.Sed").unwrap(),
            vera.path().join("RandMap.Sed")
        );
        // Maps still come from the game folder first.
        game.write("Lakes.map", b"[Basic]\n");
        vera.write("Lakes.map", b"[Basic]\n");
        assert_eq!(
            local_map_path_in(None, game.path(), Some(vera.path()), "Lakes.map").unwrap(),
            game.path().join("Lakes.map")
        );
        // A mod folder in use reads as if copied on top of the game folder:
        // its map first, the game folder's where it has none, never its seed.
        let mod_dir = TestDirectory::new("seed-lookup-mod");
        mod_dir.write("Lakes.map", b"[Basic]\n");
        mod_dir.write("RandMap.Sed", b"[RandomMap]\nSeed=3\n");
        game.write("Hills.map", b"[Basic]\n");
        let found = |name| {
            local_map_path_in(Some(mod_dir.path()), game.path(), Some(vera.path()), name).unwrap()
        };
        assert_eq!(found("Lakes"), mod_dir.path().join("Lakes.map"));
        assert_eq!(found("Hills.map"), game.path().join("Hills.map"));
        assert_eq!(found("RandMap.Sed"), vera.path().join("RandMap.Sed"));
    }
    use super::test_support::install_received_map;
    use super::*;
    use crate::assets::mix_hash::mix_hash;

    #[test]
    fn legacy_generated_source_keeps_absent_seed_identity() {
        let legacy = serde_json::json!({"kind": "generated", "seed_name": "Old.SED"});
        let source: LoadedMapSource = serde_json::from_value(legacy.clone()).unwrap();
        assert!(matches!(
            source,
            LoadedMapSource::Generated {
                seed_source: None,
                ..
            }
        ));
        assert_eq!(serde_json::to_value(source).unwrap(), legacy);
    }

    #[test]
    fn asset_map_candidates_adds_retail_map_extensions_for_stems() {
        assert_eq!(
            asset_map_candidates("mp01t2"),
            vec![
                "mp01t2".to_string(),
                "mp01t2.mmx".to_string(),
                "mp01t2.yro".to_string(),
                "mp01t2.map".to_string(),
                "mp01t2.mpr".to_string(),
                "mp01t2.yrm".to_string(),
            ]
        );
    }

    #[test]
    fn asset_map_candidates_keeps_explicit_map_names_exact() {
        assert_eq!(asset_map_candidates("MP01T2.MAP"), vec!["MP01T2.MAP"]);
    }

    const SOURCE_TEST_MAP: &[u8] = b"[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n[IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n";
    // SHA-256 independently computed from the literal raw INI buffer, not from
    // its parsed or normalized representation.
    const SOURCE_TEST_MAP_SHA256: &str =
        "889a91e6c8652968c930f40cfe998f2c0f4152d3f4b10b6af1a8c8e9a7c6e814";

    fn install_test_seed(
        directory: &Path,
        bytes: &[u8],
        limits: cache::Limits,
    ) -> Result<LoadedTransferAsset> {
        install_received_asset_with_limits(
            directory,
            bytes.to_vec(),
            &sha256_hex(bytes),
            bytes.len(),
            TransferResource::Seed,
            limits,
        )
    }

    #[test]
    fn cache_quota_preserves_active_files_and_retires_unpinned_entries() {
        let root = TestDirectory::new("cache-active-pins");
        let old = b"[RandomMap]\nSeed=1\n";
        let current = b"[RandomMap]\nSeed=2\n";
        let next = b"[RandomMap]\nSeed=3\n";
        let limits = cache::Limits {
            bytes: (old.len() * 2) as u64,
            files: 2,
        };
        let old_path = root.path().join(format!("{}.sed", sha256_hex(old)));
        std::fs::write(&old_path, old).unwrap();
        // An independent shared handle models a different running game's pin.
        let old_pin = root.path().join(format!(".{}.sed.pin", sha256_hex(old)));
        let other_game = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&old_pin)
            .unwrap();
        other_game.lock_shared().unwrap();
        let active = install_test_seed(root.path(), current, limits).unwrap();
        assert!(install_test_seed(root.path(), next, limits).is_err());
        assert_eq!(std::fs::read(&old_path).unwrap(), old);
        assert!(
            !root
                .path()
                .join(format!("{}.sed", sha256_hex(next)))
                .exists()
        );

        drop(other_game);
        let next = install_test_seed(root.path(), next, limits).unwrap();
        assert!(!old_path.exists());
        assert!(
            !old_pin.exists(),
            "eviction retires its stable lock sidecar"
        );
        let LoadedMapSource::Loose { path, .. } = &active.source else {
            panic!("cached seed");
        };
        let path = path.clone();
        assert_eq!(std::fs::read(&path).unwrap(), current);
        let serialized = serde_json::to_value(&active.source).unwrap();
        assert!(serialized.get("cache_lease").is_none());
        let restored: LoadedMapSource = serde_json::from_value(serialized).unwrap();
        assert_eq!(restored, active.source);
        let retained = active.source.clone();
        drop(active);
        let replacement = b"[RandomMap]\nSeed=4\n";
        assert!(install_test_seed(root.path(), replacement, limits).is_err());
        drop(retained);
        install_test_seed(root.path(), replacement, limits).unwrap();
        assert!(
            !path.exists(),
            "the final source clone releases its cache pin"
        );
        drop(next);
    }

    #[test]
    fn concurrent_cache_installs_cannot_overrun_exact_capacity() {
        let root = TestDirectory::new("cache-concurrent-quota");
        let first = b"[RandomMap]\nSeed=1\n";
        let second = b"[RandomMap]\nSeed=2\n";
        let limits = cache::Limits {
            bytes: first.len() as u64,
            files: 1,
        };
        let barrier = std::sync::Barrier::new(2);
        let results = std::thread::scope(|scope| {
            let one = scope.spawn(|| {
                barrier.wait();
                install_test_seed(root.path(), first, limits)
            });
            let two = scope.spawn(|| {
                barrier.wait();
                install_test_seed(root.path(), second, limits)
            });
            [one.join().unwrap(), two.join().unwrap()]
        });
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let data: Vec<_> = std::fs::read_dir(root.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "sed"))
            .collect();
        assert_eq!(data.len(), 1);
        let installed = std::fs::read(&data[0]).unwrap();
        assert!(installed == first || installed == second);
        assert!(install_test_seed(root.path(), b"[RandomMap]\nSeed=10\n", limits).is_err());
        assert_eq!(std::fs::read(&data[0]).unwrap(), installed);
        assert!(std::fs::read_dir(root.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|ext| ext == "tmp")
        }));
        let assets = AssetManager::from_loose_root_for_test(root.path());
        let loaded = results
            .iter()
            .find_map(|result| result.as_ref().ok())
            .unwrap();
        let upload = prepare_transfer_map_source(&loaded.source, &assets).unwrap();
        let replacement = if installed == first { second } else { first };
        drop(results);
        assert!(
            install_test_seed(root.path(), replacement, limits).is_err(),
            "a queued upload must retain the file after setup releases its source"
        );
        assert_eq!(
            upload
                .read(&sha256_hex(&installed), installed.len())
                .unwrap(),
            installed
        );
        install_test_seed(root.path(), replacement, limits).unwrap();
        assert!(
            !data[0].exists(),
            "completed upload releases the final lease"
        );
    }

    #[test]
    fn cache_quota_counts_foreign_files_without_deleting_or_following_them() {
        let root = TestDirectory::new("cache-foreign-files");
        let directory = root.path().join("cache");
        std::fs::create_dir(&directory).unwrap();
        let old = b"[RandomMap]\nSeed=1\n";
        let received = b"[RandomMap]\nSeed=2\n";
        let misleading = format!("{}.sed", sha256_hex(b"[RandomMap]\nSeed=3\n"));
        let old_path = directory.join(format!("{}.sed", sha256_hex(old)));
        std::fs::write(&old_path, old).unwrap();
        std::fs::write(directory.join("notes.txt"), b"keep").unwrap();
        std::fs::write(directory.join(&misleading), b"foreign").unwrap();
        std::fs::write(directory.join(".map-999999-0.tmp"), b"partial").unwrap();
        let orphan = directory.join(format!(".{}.sed.pin", "0".repeat(64)));
        std::fs::write(&orphan, []).unwrap();
        #[cfg(unix)]
        {
            root.write("outside.sed", b"outside");
            std::os::unix::fs::symlink(
                root.path().join("outside.sed"),
                directory.join(format!("{}.sed", sha256_hex(b"[RandomMap]\nSeed=4\n"))),
            )
            .unwrap();
        }
        let limits = cache::Limits {
            bytes: (received.len() + 4 + 7 + 7) as u64,
            files: 4,
        };
        install_test_seed(&directory, received, limits).unwrap();
        assert!(!old_path.exists());
        assert!(!orphan.exists());
        assert_eq!(std::fs::read(directory.join("notes.txt")).unwrap(), b"keep");
        assert_eq!(
            std::fs::read(directory.join(&misleading)).unwrap(),
            b"foreign"
        );
        assert_eq!(
            std::fs::read(directory.join(".map-999999-0.tmp")).unwrap(),
            b"partial"
        );
        assert!(
            install_test_seed(&directory, received, cache::Limits { files: 3, ..limits }).is_err(),
            "a cache hit must also respect a pre-existing over-quota directory"
        );
        #[cfg(unix)]
        {
            assert!(install_test_seed(&directory, b"[RandomMap]\nSeed=4\n", limits).is_err());
            assert_eq!(
                std::fs::read(root.path().join("outside.sed")).unwrap(),
                b"outside"
            );
        }
    }

    #[test]
    fn received_preview_checks_each_edge_before_publication() {
        let root = TestDirectory::new("received-preview-edges");
        let cache = root.path().join("NetworkMaps");
        // A 65535x1 PCX used to pass the area limit and reach GPU creation.
        let wide = crate::assets::pcx_file::encode_direct_rgb(
            u16::MAX,
            1,
            &vec![0; usize::from(u16::MAX) * 3],
        )
        .unwrap();
        assert!(wide.len() < TransferResource::Preview.limit());
        assert!(
            install_received_asset(
                &cache,
                wide.clone(),
                &sha256_hex(&wide),
                wide.len(),
                TransferResource::Preview,
            )
            .is_err()
        );
        let tall = crate::assets::pcx_file::encode_direct_rgb(1, 2049, &vec![0; 2049 * 3]).unwrap();
        assert!(
            install_received_asset(
                &cache,
                tall.clone(),
                &sha256_hex(&tall),
                tall.len(),
                TransferResource::Preview,
            )
            .is_err()
        );
        assert!(!cache.exists());
        let boundary =
            crate::assets::pcx_file::encode_direct_rgb(2048, 1, &vec![0; 2048 * 3]).unwrap();
        assert!(
            install_received_asset(
                &cache,
                boundary.clone(),
                &sha256_hex(&boundary),
                boundary.len(),
                TransferResource::Preview,
            )
            .is_ok()
        );
    }

    #[test]
    fn received_scenarios_reject_binary_and_unrelated_members_before_publication() {
        let root = TestDirectory::new("received-scenario-content");
        let cache = root.path().join("NetworkMaps");
        let mut binary_map = SOURCE_TEST_MAP.to_vec();
        binary_map.extend_from_slice(b"\0MZ\x01");
        let seed = b"[RandomMap]\nSeed=2\n\0MZ\x01".to_vec();
        let opaque_wrapper = make_new_format_mix_bytes(&[
            ("Arena.map", SOURCE_TEST_MAP),
            ("hidden.exe", b"MZ\0\x01"),
        ]);
        let unrelated_wrapper = make_new_format_mix_bytes(&[
            ("Arena.map", SOURCE_TEST_MAP),
            ("other.ini", b"[Other]\nx=1\n"),
        ]);
        let wrapped_seed = make_new_format_mix_bytes(&[("seed.sed", b"[RandomMap]\nSeed=2\n")]);
        for (bytes, resource) in [
            (binary_map, TransferResource::Map),
            (seed, TransferResource::Seed),
            (opaque_wrapper, TransferResource::Map),
            (unrelated_wrapper, TransferResource::Map),
            (wrapped_seed, TransferResource::Seed),
        ] {
            assert!(
                install_received_asset(
                    &cache,
                    bytes.clone(),
                    &sha256_hex(&bytes),
                    bytes.len(),
                    resource,
                )
                .is_err()
            );
        }
        assert!(!cache.exists(), "type rejection precedes cache publication");
        let legacy_seed = b"[RandomMap]\r\nSeed=2\r\n; caf\xe9\t\r\n";
        assert!(
            install_received_asset(
                &cache,
                legacy_seed.to_vec(),
                &sha256_hex(legacy_seed),
                legacy_seed.len(),
                TransferResource::Seed,
            )
            .is_ok(),
            "legacy high-bit text remains supported"
        );
    }

    #[test]
    fn received_wrapper_accounts_for_layout_and_keeps_exact_identity() {
        let root = TestDirectory::new("received-wrapper-layout");
        let bytes = make_new_format_mix_bytes(&[
            (
                "description.ini",
                b"[MultiMaps]\n1=Arena\n[Arena]\nDescription=Test\n",
            ),
            ("Arena.map", SOURCE_TEST_MAP),
        ]);
        let mut checksummed = bytes.clone();
        checksummed[2..4].copy_from_slice(&1u16.to_le_bytes());
        let checksum = crate::util::sha1::sha1_bytes(&checksummed[34..]);
        checksummed.extend_from_slice(&checksum);
        let digest = sha256_hex(&checksummed);
        let loaded =
            install_received_map(root.path(), checksummed.clone(), &digest, checksummed.len())
                .unwrap();
        assert_eq!(
            loaded.source.physical_identity(),
            Some((checksummed.len(), digest.as_str()))
        );
        let LoadedMapSource::Loose { path, .. } = &loaded.source else {
            panic!("cached wrapper");
        };
        assert_eq!(std::fs::read(path).unwrap(), checksummed);

        let mut overlap = bytes.clone();
        overlap[26..30].copy_from_slice(&0u32.to_le_bytes());
        let mut duplicate_id = bytes.clone();
        duplicate_id.copy_within(10..14, 22);
        let mut trailer = bytes;
        trailer.extend_from_slice(b"MZunused");
        let mut bad_checksum = checksummed;
        *bad_checksum.last_mut().unwrap() ^= 1;
        for invalid in [overlap, duplicate_id, trailer, bad_checksum] {
            assert!(
                install_received_map(
                    root.path(),
                    invalid.clone(),
                    &sha256_hex(&invalid),
                    invalid.len()
                )
                .is_err()
            );
        }
    }

    #[test]
    fn transferred_named_retail_wrappers_keep_description_alignment_and_checksum() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let root = TestDirectory::new("received-retail-wrappers");
        for name in ["Hills.mmx", "Dustbowl.mmx", "RiverRam.yro"] {
            let bytes = assets
                .get(name)
                .unwrap_or_else(|| panic!("missing retail {name}"));
            let digest = sha256_hex(&bytes);
            let loaded = install_received_map(root.path(), bytes.clone(), &digest, bytes.len())
                .unwrap_or_else(|error| panic!("retail {name}: {error}"));
            assert_eq!(
                loaded.source.physical_identity(),
                Some((bytes.len(), digest.as_str()))
            );
        }
    }

    #[test]
    fn seed_and_preview_keep_distinct_verified_loader_formats() {
        let root = TestDirectory::new("received-generated-assets");
        let mut options = crate::map::rmg::RmgOptions::default();
        options.seed = 1234;
        options.normalize();
        let bytes = options.to_sed_bytes(false);
        let digest = sha256_hex(&bytes);
        let loaded = install_received_asset(
            root.path(),
            bytes.clone(),
            &digest,
            bytes.len(),
            TransferResource::Seed,
        )
        .unwrap();
        let TransferContent::Scenario(ini) = loaded.content else {
            panic!("seed INI");
        };
        let mut decoded = crate::map::rmg::RmgOptions::default();
        decoded.apply_sed(&ini, false);
        decoded.normalize();
        assert_eq!(decoded, options);
        let LoadedMapSource::Loose { path, .. } = loaded.source else {
            panic!("physical seed");
        };
        assert_eq!(path.extension().unwrap(), "sed");
        assert!(
            install_received_map(root.path(), bytes.clone(), &digest, bytes.len()).is_err(),
            "a seed is never an authored map"
        );
        let preview =
            crate::assets::pcx_file::encode_direct_rgb(2, 1, &[255, 0, 0, 0, 255, 0]).unwrap();
        let loaded = install_received_asset(
            root.path(),
            preview.clone(),
            &sha256_hex(&preview),
            preview.len(),
            TransferResource::Preview,
        )
        .unwrap();
        let TransferContent::Preview(image) = loaded.content else {
            panic!("preview pixels");
        };
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.rgba, [255, 0, 0, 255, 0, 255, 0, 255]);
    }

    #[test]
    fn network_logical_names_reject_paths_but_retained_cache_and_local_paths_still_load() {
        assert!(validate_network_map_name("Maps/Arena.map").is_ok());
        assert!(validate_network_map_name("RandMap.Sed").is_ok());
        assert!(validate_network_map_name("../outside.map").is_err());
        assert!(validate_network_map_name("C:\\outside.map").is_err());
        assert!(validate_network_map_name("Maps/com1.map").is_err());
        assert!(validate_network_map_name("Maps/Comet.map").is_ok());
        let root = TestDirectory::new("network-logical-map-name");
        let cache = root.path().join("NetworkMaps");
        let loaded = install_received_map(
            &cache,
            SOURCE_TEST_MAP.to_vec(),
            SOURCE_TEST_MAP_SHA256,
            SOURCE_TEST_MAP.len(),
        )
        .unwrap();
        let LoadedMapSource::Loose { path, .. } = loaded.source else {
            panic!("retained physical source");
        };
        let absolute = path.to_str().unwrap();
        assert!(validate_network_map_name(absolute).is_err());
        let assets = AssetManager::from_loose_root_for_test(root.path());
        let retained = load_network_map_ini(root.path(), absolute, &assets).unwrap();
        assert_eq!(
            retained.source.physical_identity(),
            Some((SOURCE_TEST_MAP.len(), SOURCE_TEST_MAP_SHA256))
        );
        assert!(load_map_by_name_or_path_with_assets(root.path(), absolute, &assets).is_ok());
    }

    #[test]
    fn a_saved_map_path_counts_only_inside_a_map_folder() {
        let game = Path::new("/games/ra2");
        let folder = |path: &Path| saved_map_folder_in([game], path);
        assert_eq!(folder(&game.join("Maps/mission.map")), Some(game));
        assert_eq!(folder(&game.join("../outside/mission.map")), None);
        assert_eq!(folder(Path::new("/games/ra2-other/mission.map")), None);
        assert_eq!(folder(game), None, "the folder itself is no map");
        for unnamed in ["", "/"] {
            let path = Path::new(unnamed).join("mission.map");
            assert_eq!(
                saved_map_folder_in([Path::new(unnamed)], &path),
                None,
                "an unset folder or a root holds nothing: {unnamed:?}"
            );
        }
    }

    #[test]
    fn network_effective_digest_uses_existing_include_expansion_and_rejects_missing_closure() {
        for ares_way in [false, true] {
            let _ares = crate::rules::ares_switch::read_ares_way(ares_way);
            let root = TestDirectory::new("network-map-includes");
            let mut bytes = SOURCE_TEST_MAP.to_vec();
            bytes.extend_from_slice(b"\n[#include]\n0=child.ini\n");
            root.write("Shared.map", &bytes);
            root.write("child.ini", b"[General]\nBuildSpeed=.5\n");
            let a = AssetManager::from_loose_root_for_test(root.path());
            let first = load_network_map_ini(root.path(), "Shared.map", &a).unwrap();
            root.write("child.ini", b"[General]\nBuildSpeed=.7\n");
            let b = AssetManager::from_loose_root_for_test(root.path());
            let second = load_network_map_ini(root.path(), "Shared.map", &b).unwrap();
            assert_eq!(
                first.source.physical_identity(),
                second.source.physical_identity()
            );
            assert_eq!(
                effective_network_map_digest(&first.ini, false, false)
                    != effective_network_map_digest(&second.ini, false, false),
                ares_way
            );
            std::fs::remove_file(root.path().join("child.ini")).unwrap();
            let c = AssetManager::from_loose_root_for_test(root.path());
            assert_eq!(
                load_network_map_ini(root.path(), "Shared.map", &c).is_err(),
                ares_way
            );
            if ares_way {
                root.write("child.ini", SOURCE_TEST_MAP);
                let bytes = b"[#include]\n0=child.ini\n".to_vec();
                let loaded = install_received_map(
                    &root.path().join("cache"),
                    bytes.clone(),
                    &sha256_hex(&bytes),
                    bytes.len(),
                )
                .unwrap();
                let assets = AssetManager::from_loose_root_for_test(root.path());
                let loaded =
                    resolve_network_map_ini(loaded, &assets, false, "Included.map").unwrap();
                validate_network_map_content(&loaded.ini, false).unwrap();
            }
        }
    }

    #[test]
    fn received_map_is_verified_before_atomic_publication_and_cache_reuse() {
        let root = TestDirectory::new("received-map");
        let cache = root.path().join("NetworkMaps");
        assert!(
            install_received_map(
                &cache,
                SOURCE_TEST_MAP.to_vec(),
                &"0".repeat(64),
                SOURCE_TEST_MAP.len()
            )
            .is_err()
        );
        assert!(!cache.exists());
        assert!(
            install_received_map(&cache, b"not a map".to_vec(), &sha256_hex(b"not a map"), 9)
                .is_err()
        );
        assert!(!cache.exists());
        let loaded = install_received_map(
            &cache,
            SOURCE_TEST_MAP.to_vec(),
            SOURCE_TEST_MAP_SHA256,
            SOURCE_TEST_MAP.len(),
        )
        .unwrap();
        assert_eq!(
            loaded.source.physical_identity(),
            Some((SOURCE_TEST_MAP.len(), SOURCE_TEST_MAP_SHA256))
        );
        let LoadedMapSource::Loose { path, .. } = &loaded.source else {
            panic!("local file");
        };
        assert!(path.is_absolute());
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            format!("{SOURCE_TEST_MAP_SHA256}.map")
        );
        let load = || {
            load_received_asset(
                &cache,
                SOURCE_TEST_MAP_SHA256,
                SOURCE_TEST_MAP.len(),
                TransferResource::Map,
            )
        };
        assert!(load().is_ok());
        std::fs::write(path, b"damaged").unwrap();
        assert!(load().is_err());
        install_received_map(
            &cache,
            SOURCE_TEST_MAP.to_vec(),
            SOURCE_TEST_MAP_SHA256,
            SOURCE_TEST_MAP.len(),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_dir(&cache)
                .unwrap()
                .filter(|entry| {
                    let path = entry.as_ref().unwrap().path();
                    path.file_name().unwrap() != ".vera20k-cache.lock"
                        && !path.extension().is_some_and(|ext| ext == "pin")
                })
                .count(),
            1,
            "no staged files escape"
        );
    }

    #[test]
    fn asset_aware_map_readers_use_the_selected_mod_without_global_setup() {
        use crate::assets::asset_manager::MediaArchiveMode;

        let base = crate::assets::asset_manager::test_support::audio_test_install("map-base");
        let selected_mod = TestDirectory::new("map-selected-mod");
        let base_map = [SOURCE_TEST_MAP, b"[Basic]\nName=Base arena\n"].concat();
        let mod_map = [SOURCE_TEST_MAP, b"[Basic]\nName=Mod arena\n"].concat();
        std::fs::write(base.path().join("Arena.map"), &base_map).unwrap();
        selected_mod.write("ARENA.MAP", &mod_map);
        let assets = AssetManager::with_mod(
            base.path(),
            Some(selected_mod.path()),
            MediaArchiveMode::STOCK_DIGITAL,
        )
        .unwrap();

        let gameplay =
            load_map_by_name_or_path_with_assets(base.path(), "Arena.map", &assets).unwrap();
        let metadata =
            load_map_ini_by_name_or_path_with_assets(base.path(), "Arena.map", &assets).unwrap();
        let network = load_network_map_ini(base.path(), "Arena.map", &assets).unwrap();
        let identity = map_source_identity(base.path(), "Arena.map", &assets).unwrap();
        for ini in [&gameplay.map.ini, &metadata.ini, &network.ini] {
            assert_eq!(
                ini.section_or_empty("Basic").read_name("Name", 128),
                Some("Mod arena")
            );
        }
        let digest = sha256_hex(&mod_map);
        for source in [
            &gameplay.source,
            &metadata.source,
            &network.source,
            &identity,
        ] {
            let LoadedMapSource::Loose { path, .. } = source else {
                panic!("the selected mod's loose map must win");
            };
            assert_eq!(path.parent(), Some(selected_mod.path()));
            assert_eq!(
                source.physical_identity(),
                Some((mod_map.len(), digest.as_str()))
            );
        }

        let original = AssetManager::new(base.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
        let metadata =
            load_map_ini_by_name_or_path_with_assets(base.path(), "Arena.map", &original).unwrap();
        assert_eq!(
            metadata
                .ini
                .section_or_empty("Basic")
                .read_name("Name", 128),
            Some("Base arena")
        );
    }

    #[test]
    fn ini_directives_map_gameplay_and_metadata_follow_build_capability() {
        for ares_way in [false, true] {
            let _ares = crate::rules::ares_switch::read_ares_way(ares_way);
            let dir = TestDirectory::new("map-ini-directives");
            let root = [
                SOURCE_TEST_MAP,
                b"[#include]\n0=rules-extra.ini\n[Unit]\n$Inherits=Base\nOwn=map\n",
            ]
            .concat();
            dir.write("Arena.map", &root);
            dir.write(
                "rules-extra.ini",
                b"[Basic]\nName=Included name\n[Base]\nStrength=250\nOwn=parent\n",
            );
            let assets = AssetManager::from_loose_root_for_test(dir.path());
            let loaded =
                load_map_by_name_or_path_with_assets(dir.path(), "Arena.map", &assets).unwrap();
            let metadata =
                load_map_ini_by_name_or_path_with_assets(dir.path(), "Arena.map", &assets).unwrap();
            assert_eq!(loaded.map.ini.content_hash(), metadata.ini.content_hash());
            let ares_include = ares_way;
            assert_eq!(
                loaded
                    .map
                    .ini
                    .section_or_empty("Unit")
                    .read_int("Strength", 0),
                if ares_include { 250 } else { 0 }
            );
            assert_eq!(
                loaded
                    .map
                    .ini
                    .section_or_empty("Unit")
                    .read_name("Own", 128),
                Some("map")
            );
            assert_eq!(
                loaded
                    .map
                    .ini
                    .section_or_empty("Basic")
                    .read_name("Name", 128),
                ares_include.then_some("Included name")
            );
            assert_eq!(
                serde_json::to_value(&loaded.source).unwrap(),
                serde_json::to_value(&metadata.source).unwrap()
            );
        }
    }

    #[test]
    fn loose_map_source_retains_consumed_bytes_after_file_changes() {
        let directory = TestDirectory::new("loose-source");
        directory.write("Arena.map", SOURCE_TEST_MAP);
        let loaded = local_map_path(directory.path(), "Arena")
            .and_then(|path| read_loose_map_input(&path))
            .and_then(decode_map)
            .expect("resolve extension and load loose map");
        directory.write("Arena.map", b"file replaced after map load");

        assert_eq!(loaded.map.header.theater, "TEMPERATE");
        assert_eq!(
            loaded.source,
            LoadedMapSource::Loose {
                cache_lease: Default::default(),
                path: directory.path().join("Arena.map"),
                payload_len: SOURCE_TEST_MAP.len(),
                source_sha256: SOURCE_TEST_MAP_SHA256.to_string(),
            }
        );
    }

    #[test]
    fn asset_fallback_keeps_the_loose_wrappers_path_and_bytes() {
        let direct = TestDirectory::new("empty-direct-root");
        let loose = TestDirectory::new("asset-wrapper");
        let wrapper = make_new_format_mix_bytes(&[("Arena.map", SOURCE_TEST_MAP)]);
        loose.write("Arena.mmx", &wrapper);
        let assets = AssetManager::from_loose_root_for_test(loose.path());
        let name = "ARENA.MMX";
        let metadata =
            load_map_ini_by_name_or_path_with_assets(direct.path(), name, &assets).unwrap();
        let digest = sha256_hex(&wrapper);
        assert_eq!(
            metadata.source,
            LoadedMapSource::Loose {
                path: loose.path().join("Arena.mmx"),
                payload_len: wrapper.len(),
                source_sha256: digest.clone(),
                cache_lease: Default::default(),
            }
        );

        let gameplay = load_map_by_name_or_path_with_assets(direct.path(), name, &assets).unwrap();
        assert_eq!(gameplay.map.header.theater, "TEMPERATE");
        assert_eq!(gameplay.map.ini.content_hash(), metadata.ini.content_hash());
        assert_eq!(gameplay.source, metadata.source);

        // The same map as plain INI is a different file from the selected wrapper.
        let upload = prepare_transfer_map_source(&metadata.source, &assets).unwrap();
        loose.write("Arena.mmx", SOURCE_TEST_MAP);
        assert!(upload.read(&digest, wrapper.len()).is_err());
    }

    #[test]
    fn loose_wrapped_map_hashes_container_and_preserves_map_entry_selection() {
        let directory = TestDirectory::new("wrapped-source");
        // Deliberately larger than the actual map: the shared selector must
        // skip the description even though it sorts entries by descending size.
        let description = format!("[MultiMaps]\n1={}\n", "description".repeat(30));
        let archive = make_new_format_mix_bytes(&[
            ("description.ini", description.as_bytes()),
            ("Arena.MAP", SOURCE_TEST_MAP),
        ]);
        directory.write("Arena.mmx", &archive);
        directory.write("broken.mmx", b"not a map");
        let loaded = try_load_mmx(directory.path(), &["broken.mmx", "Arena.mmx"])
            .expect("skip failed candidate and load wrapped map");

        assert_eq!(loaded.map.header.theater, "TEMPERATE");
        assert_eq!(
            loaded.source,
            LoadedMapSource::Loose {
                cache_lease: Default::default(),
                path: directory.path().join("Arena.mmx"),
                payload_len: archive.len(),
                source_sha256: sha256_hex(&archive),
            }
        );
        assert_ne!(sha256_hex(&archive), SOURCE_TEST_MAP_SHA256);
        let metadata = read_map_ini_for_metadata(&directory.path().join("Arena.mmx"))
            .expect("metadata scan selects the same map entry");
        assert!(metadata.section("Map").is_some());
        let direct = map_file::load_from_path(&directory.path().join("Arena.mmx"))
            .expect("shared disk loader retains wrapped map support");
        assert_eq!(direct.header.theater, loaded.map.header.theater);
    }

    #[test]
    fn metadata_selection_does_not_require_decodable_terrain() {
        let directory = TestDirectory::new("metadata-only");
        let ini = b"[Map]\nTheater=SNOW\n[Basic]\nName=Metadata only\n";
        directory.write("Metadata.map", ini);
        directory.write(
            "Metadata.mmx",
            &make_new_format_mix_bytes(&[("Metadata.MAP", ini)]),
        );
        for name in ["Metadata.map", "Metadata.mmx"] {
            let path = directory.path().join(name);
            let metadata = read_map_ini_for_metadata(&path).expect("lightweight metadata");
            assert_eq!(
                metadata.section("Map").unwrap().get_for_test("Theater"),
                Some("SNOW")
            );
            assert!(map_file::load_from_path(&path).is_err());
        }
    }

    #[test]
    fn archived_map_source_hashes_extracted_entry_not_container() {
        let directory = TestDirectory::new("mix-source");
        let archive = make_new_format_mix_bytes(&[("Arena.MAP", SOURCE_TEST_MAP)]);
        directory.write("Arena.YRO", &archive);
        let mut assets = AssetManager::from_loose_root_for_test(directory.path());
        assert!(
            assets
                .register_loose_yro_archive(&directory.path().join("Arena.YRO"))
                .expect("register scenario archive")
        );
        let loaded = load_map_by_name_or_path_with_assets(directory.path(), "Arena.MAP", &assets)
            .expect("load actual archived map entry");

        assert_eq!(loaded.map.header.theater, "TEMPERATE");
        assert_eq!(
            loaded.source,
            LoadedMapSource::Mix {
                logical_name: "Arena.MAP".to_string(),
                source_archive: "Arena.YRO".to_string(),
                entry_id: mix_hash("Arena.MAP"),
                payload_len: SOURCE_TEST_MAP.len(),
                source_sha256: SOURCE_TEST_MAP_SHA256.to_string(),
            }
        );
        assert_ne!(sha256_hex(&archive), SOURCE_TEST_MAP_SHA256);
    }

    #[test]
    fn map_source_manifest_preserves_actual_mix_lookup_facts() {
        let source = LoadedMapSource::Mix {
            logical_name: "Fight.MAP".to_string(),
            source_archive: "multimd.mix".to_string(),
            entry_id: 0x9306_F050_u32 as i32,
            payload_len: 91_254,
            source_sha256: "d751dce7cd3611077e9228c33235f39c71681fff6ac08ca1f716d963ad6ce070"
                .to_string(),
        };
        let json = serde_json::to_value(source).expect("serialize source");

        assert_eq!(json["kind"], "mix");
        assert_eq!(json["logical_name"], "Fight.MAP");
        assert_eq!(json["source_archive"], "multimd.mix");
        assert_eq!(json["entry_id"], 0x9306_F050_u32 as i32);
        assert_eq!(json["payload_len"], 91_254);
        assert_eq!(
            json["source_sha256"],
            "d751dce7cd3611077e9228c33235f39c71681fff6ac08ca1f716d963ad6ce070"
        );
    }
}
