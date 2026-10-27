//! Resolve map names and paths through loose files and the retained asset VFS.
//!
//! Owns source selection and the identity of the exact bytes consumed by the
//! parser. Frontend discovery and tools use the same resolution path.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::assets::asset_manager::AssetManager;
use crate::map::map_file::{self, MapFile};
use crate::rules::ini_parser::IniFile;
use crate::util::sha256::sha256_hex;

const MAP_EXTENSIONS: [&str; 5] = ["mmx", "yro", "map", "mpr", "yrm"];
pub const MAX_TRANSFER_MAP_BYTES: usize = 8 * 1024 * 1024;
mod cache;
pub use cache::CacheLease;

/// Exact source whose bytes were parsed into the active map.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoadedMapSource {
    Loose {
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

/// A multiplayer map name is a VFS-relative logical identity, never a peer's
/// host path. Check both separator spellings independently of the local OS so
/// a LAN/online name has the same admission on every client. Absolute cache
/// paths remain internal `LoadedMapSource` values and do not pass this boundary.
pub fn validate_network_map_name(name: &str) -> Result<()> {
    anyhow::ensure!(
        !name.is_empty()
            && name.len() <= 1024
            && !name.chars().any(char::is_control)
            && !name.contains(':')
            && name
                .split(['/', '\\'])
                .all(|part| !part.is_empty() && part != "." && part != ".."),
        "invalid logical multiplayer map name"
    );
    Ok(())
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
    map_file::ini_from_file_bytes(crate::util::file_system::read(path).ok()?).ok()
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

fn local_map_path(ra2_dir: &Path, map_name: &str) -> Result<PathBuf> {
    let direct = PathBuf::from(map_name);
    if direct.is_absolute() && crate::util::file_system::exists(&direct) {
        return Ok(direct);
    }
    let in_ra2 = ra2_dir.join(map_name);
    if crate::util::file_system::exists(&in_ra2) {
        return Ok(in_ra2);
    }
    for ext in MAP_EXTENSIONS {
        let candidate = ra2_dir.join(format!("{map_name}.{ext}"));
        if crate::util::file_system::exists(&candidate) {
            return Ok(candidate);
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
    let source = LoadedMapSource::Loose {
        cache_lease: Default::default(),
        path: path.to_path_buf(),
        payload_len: bytes.len(),
        source_sha256: sha256_hex(&bytes),
    };
    Ok(MapInput {
        bytes: Cow::Owned(bytes),
        source,
    })
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
        ini: map_file::ini_from_file_bytes(input.bytes.into_owned())
            .unwrap_or_else(|_| IniFile::empty()),
        source: input.source,
    })
}

fn expand_map_ini(
    ini: IniFile,
    source: &LoadedMapSource,
    assets: &AssetManager,
) -> Result<IniFile> {
    expand_map_ini_with_options(ini, source, assets, false, None)
}

fn expand_map_ini_with_options(
    ini: IniFile,
    source: &LoadedMapSource,
    assets: &AssetManager,
    strict: bool,
    logical_name: Option<&str>,
) -> Result<IniFile> {
    if !(crate::rules::ARES_ENABLED || crate::rules::PHOBOS_ENABLED) {
        return Ok(ini);
    }
    let name = match source {
        LoadedMapSource::Loose { path, .. } => path.to_string_lossy().into_owned(),
        LoadedMapSource::Mix { logical_name, .. } => logical_name.clone(),
        _ => "<map>".to_owned(),
    };
    let name = logical_name.unwrap_or(&name);
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
        loaded.ini = expand_map_ini_with_options(
            loaded.ini,
            &loaded.source,
            assets,
            true,
            Some(logical_name),
        )?;
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

pub fn effective_network_map_digest(ini: &IniFile, seed: bool) -> String {
    if seed {
        let mut options = crate::map::rmg::RmgOptions::default();
        options.apply_sed(ini);
        options.normalize();
        return IniFile::from_bytes(&options.to_sed_bytes())
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
        map_file::ini_from_file_bytes(input.bytes.into_owned())?
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
/// policy. Preserve the existing fallback on a failed loose parse.
fn load_map_input_with_assets<T>(
    ra2_dir: &Path,
    map_name: &str,
    assets: &AssetManager,
    limit: Option<usize>,
    decode: impl Fn(MapInput<'_>) -> Result<T>,
) -> Result<T> {
    let local = local_map_path(ra2_dir, map_name)
        .and_then(|path| read_loose_map_input_bounded(&path, limit))
        .and_then(&decode);
    match local {
        Ok(value) => Ok(value),
        Err(local_err) => {
            for candidate in asset_map_candidates(map_name) {
                if let Some(resolved) = assets.resolve_ref(&candidate) {
                    anyhow::ensure!(
                        limit.is_none_or(|limit| resolved.bytes.len() <= limit),
                        "map exceeds transfer size limit"
                    );
                    let source = LoadedMapSource::Mix {
                        logical_name: candidate,
                        source_archive: resolved.source_archive.to_string(),
                        entry_id: resolved.entry_id,
                        payload_len: resolved.bytes.len(),
                        source_sha256: sha256_hex(resolved.bytes),
                    };
                    return decode(MapInput {
                        bytes: Cow::Borrowed(resolved.bytes),
                        source,
                    });
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
        options.apply_sed(&ini);
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
    #[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
    use std::io::Write;
    #[cfg(not(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown")))]
    use std::sync::atomic::{AtomicU64, Ordering};
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
        static NEXT_STAGE: AtomicU64 = AtomicU64::new(0);
        let (stage, mut file) = loop {
            let id = NEXT_STAGE.fetch_add(1, Ordering::Relaxed);
            let stage = cache
                .directory()
                .join(format!(".map-{}-{id}.tmp", std::process::id()));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&stage)
            {
                Ok(file) => break (stage, file),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        };
        let result = (|| -> std::io::Result<()> {
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&stage, &path)
        })();
        if result.is_err() {
            let _ = crate::util::file_system::remove_file(&stage);
        }
        result?;
    }
    let source = LoadedMapSource::Loose {
        cache_lease: cache.pin(&path)?,
        path,
        payload_len: length,
        source_sha256: digest.to_owned(),
    };
    Ok(LoadedTransferAsset { source, content })
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

/// Existing authored-map entry points share the typed asset installer.
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
        unreachable!()
    };
    Ok(LoadedMapIni { source, ini })
}
pub fn load_received_map(directory: &Path, digest: &str, length: usize) -> Result<LoadedMapIni> {
    let LoadedTransferAsset {
        source,
        content: TransferContent::Scenario(ini),
    } = load_received_asset(directory, digest, length, TransferResource::Map)?
    else {
        unreachable!()
    };
    Ok(LoadedMapIni { source, ini })
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

#[cfg(test)]
mod tests {
    use super::test_support::{TestDirectory, make_new_format_mix_bytes};
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
        let bytes = options.to_sed_bytes();
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
        decoded.apply_sed(&ini);
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
    fn network_effective_digest_uses_existing_include_expansion_and_rejects_missing_closure() {
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
            effective_network_map_digest(&first.ini, false)
                != effective_network_map_digest(&second.ini, false),
            crate::rules::ARES_ENABLED
        );
        std::fs::remove_file(root.path().join("child.ini")).unwrap();
        let c = AssetManager::from_loose_root_for_test(root.path());
        assert_eq!(
            load_network_map_ini(root.path(), "Shared.map", &c).is_err(),
            crate::rules::ARES_ENABLED
        );
        if crate::rules::ARES_ENABLED {
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
            let loaded = resolve_network_map_ini(loaded, &assets, false, "Included.map").unwrap();
            validate_network_map_content(&loaded.ini, false).unwrap();
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
        assert!(load_received_map(&cache, SOURCE_TEST_MAP_SHA256, SOURCE_TEST_MAP.len()).is_ok());
        std::fs::write(path, b"damaged").unwrap();
        assert!(load_received_map(&cache, SOURCE_TEST_MAP_SHA256, SOURCE_TEST_MAP.len()).is_err());
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
    fn ini_directives_map_gameplay_and_metadata_follow_build_capability() {
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
        let ares_include = crate::rules::ARES_ENABLED;
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
