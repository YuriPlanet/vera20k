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

/// Exact source whose bytes were parsed into the active map.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoadedMapSource {
    Loose {
        path: PathBuf,
        payload_len: usize,
        /// Hash of the consumed file buffer, including the wrapper for .mmx.
        source_sha256: String,
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
    map_file::ini_from_file_bytes(std::fs::read(path).ok()?).ok()
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
    if direct.is_absolute() && direct.exists() {
        return Ok(direct);
    }
    let in_ra2 = ra2_dir.join(map_name);
    if in_ra2.exists() {
        return Ok(in_ra2);
    }
    for ext in MAP_EXTENSIONS {
        let candidate = ra2_dir.join(format!("{map_name}.{ext}"));
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(anyhow::anyhow!(
        "Map '{}' not found (checked the retail root and .mmx/.yro/.map/.mpr/.yrm variants)",
        map_name
    ))
}

fn read_loose_map_input(path: &Path) -> Result<MapInput<'static>> {
    let bytes = std::fs::read(path)?;
    let source = LoadedMapSource::Loose {
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
    if !crate::rules::ARES_ENABLED {
        return Ok(ini);
    }
    let name = match source {
        LoadedMapSource::Loose { path, .. } => path.to_string_lossy().into_owned(),
        LoadedMapSource::Mix { logical_name, .. } => logical_name.clone(),
        _ => "<map>".to_owned(),
    };
    let (expanded, _) = crate::rules::retail_sources::expand_ini_with_assets(
        assets,
        ini,
        &name,
        Default::default(),
    )
    .map_err(anyhow::Error::msg)?;
    Ok(expanded.ini)
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
    decode: impl Fn(MapInput<'_>) -> Result<T>,
) -> Result<T> {
    let local = local_map_path(ra2_dir, map_name)
        .and_then(|path| read_loose_map_input(&path))
        .and_then(&decode);
    match local {
        Ok(value) => Ok(value),
        Err(local_err) => {
            for candidate in asset_map_candidates(map_name) {
                if let Some(resolved) = assets.resolve_ref(&candidate) {
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
    load_map_input_with_assets(ra2_dir, map_name, assets, |input| {
        decode_map_with_assets(input, assets)
    })
}

pub fn load_map_ini_by_name_or_path_with_assets(
    ra2_dir: &Path,
    map_name: &str,
    assets: &AssetManager,
) -> Result<LoadedMapIni> {
    load_map_input_with_assets(ra2_dir, map_name, assets, |input| {
        decode_metadata_with_assets(input, assets)
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
        if path.exists() {
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
        assert_eq!(
            loaded
                .map
                .ini
                .section_or_empty("Unit")
                .read_int("Strength", 0),
            if crate::rules::ARES_ENABLED { 250 } else { 0 }
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
            crate::rules::ARES_ENABLED.then_some("Included name")
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
