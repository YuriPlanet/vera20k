//! Map file discovery and loading utilities.
//!
//! Scans the RA2 directory for available maps and loads them from disk.
//! Split from `loading::init_helpers` for file-size limits.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::app::loading::init::MapMenuEntry;
use crate::assets::asset_manager::AssetManager;
use crate::assets::csf_file::CsfFile;
use crate::assets::mix_archive::MixArchive;
use crate::map::briefing::BriefingSection;
use crate::map::map_file::{self, MapFile};
use crate::map::preview::PreviewSection;
use crate::map::waypoints::DEFAULT_SKIRMISH_PLAYER_CAPACITY;
use crate::rules::ini_parser::IniFile;
use crate::map::skirmish_scenarios::{
    PktEntryFields, SkirmishScenarioRecord, SkirmishScenarioSource, parse_game_mode_list,
};
use crate::util::config::GameConfig;

/// Names the loose wildcard scans skip. Both are exclusions, not prerequisites:
/// the archived `MISSIONSMD.PKT` is already consumed as the first source, and
/// `MISSIONS.YRO` holds campaign missions that never belong in the multiplayer
/// chooser.
const MISSIONS_MD_PKT_FILE_NAME: &str = "MISSIONSMD.PKT";
const MISSIONS_YRO_FILE_NAME: &str = "MISSIONS.YRO";

/// Exact source whose bytes were parsed into the active map.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum LoadedMapSource {
    Loose {
        path: PathBuf,
        payload_len: usize,
    },
    Mix {
        logical_name: String,
        source_archive: String,
        entry_id: i32,
        payload_len: usize,
    },
    Generated {
        seed_name: String,
    },
    LegacyFallback {
        label: String,
    },
}

/// Parsed map paired with the exact source consumed by the parser.
pub(crate) struct LoadedMap {
    pub map: MapFile,
    pub source: LoadedMapSource,
}

/// List available maps in the RA2 directory for the main-menu map selector.
///
/// Includes `.mmx`, `.map`, and `.mpr` files (case-insensitive), with light
/// metadata extracted from `[Basic]` when available.
pub fn list_available_maps() -> Result<Vec<MapMenuEntry>> {
    let config: GameConfig = GameConfig::load()?;
    let ra2_dir: PathBuf = config.paths.ra2_dir;
    let mut maps: Vec<MapMenuEntry> = Vec::new();
    for entry in std::fs::read_dir(ra2_dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !retail_wildcard_file(&path) {
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let ext = ext.to_ascii_lowercase();
        if matches!(ext.as_str(), "mmx" | "yro" | "map" | "mpr" | "yrm") {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                maps.push(read_map_menu_entry(&path, name));
            }
        }
    }
    // No sort: the original game displays maps in source-enumeration order and
    // never alphabetizes. read_dir order is filesystem-dependent, but matching
    // the no-sort behavior is what parity requires here.
    Ok(maps)
}

/// Populate the native Choose Map sources through the retained process VFS.
///
/// Loose YRO archives become registered while this scan runs and therefore
/// remain available when the selected scenario is loaded later.
pub(crate) fn list_skirmish_scenario_records_with_assets(
    ra2_dir: &Path,
    assets: &mut AssetManager,
    csf: Option<&CsfFile>,
) -> Result<Vec<SkirmishScenarioRecord>> {
    list_skirmish_scenario_records_from_sources(ra2_dir, Some(assets), csf)
}

fn list_skirmish_scenario_records_from_sources(
    ra2_dir: &Path,
    mut assets: Option<&mut AssetManager>,
    csf: Option<&CsfFile>,
) -> Result<Vec<SkirmishScenarioRecord>> {
    let mut records = Vec::new();

    if let Some(assets) = assets.as_deref() {
        if let Some(pkt) = assets
            .get_ref("MISSIONSMD.PKT")
            .and_then(|bytes| IniFile::from_bytes(bytes).ok())
        {
            append_pkt_records(
                &mut records,
                &pkt,
                SkirmishScenarioSource::MissionsMdPkt,
                csf,
                PktTitleStyle::Verbatim,
                |file_name| {
                    assets
                        .get_ref(file_name)
                        .and_then(|bytes| IniFile::from_bytes(bytes).ok())
                },
            );
        }
    }

    append_loose_pkt_records(&mut records, ra2_dir, assets.as_deref(), csf)?;
    append_loose_yro_records(&mut records, ra2_dir, assets.as_deref_mut(), csf)?;
    append_loose_yrm_records(&mut records, ra2_dir)?;

    Ok(records)
}

fn append_loose_pkt_records(
    records: &mut Vec<SkirmishScenarioRecord>,
    ra2_dir: &Path,
    assets: Option<&AssetManager>,
    csf: Option<&CsfFile>,
) -> Result<()> {
    for (path, file_name) in loose_files_with_extension(ra2_dir, "pkt")? {
        // The wildcard scan would otherwise re-list every archived row a second
        // time from an extracted copy sitting beside the executable.
        if file_name.eq_ignore_ascii_case(MISSIONS_MD_PKT_FILE_NAME) {
            continue;
        }
        let Some(pkt) = read_ini_file(&path) else {
            continue;
        };
        append_pkt_records(
            records,
            &pkt,
            SkirmishScenarioSource::LoosePkt(file_name),
            csf,
            PktTitleStyle::Verbatim,
            |map_file| {
                read_map_ini_for_metadata(&ra2_dir.join(map_file)).or_else(|| {
                    assets
                        .and_then(|assets| assets.get_ref(map_file))
                        .and_then(|bytes| IniFile::from_bytes(bytes).ok())
                })
            },
        );
    }
    Ok(())
}

fn append_loose_yro_records(
    records: &mut Vec<SkirmishScenarioRecord>,
    ra2_dir: &Path,
    mut assets: Option<&mut AssetManager>,
    csf: Option<&CsfFile>,
) -> Result<()> {
    for (path, file_name) in loose_files_with_extension(ra2_dir, "yro")? {
        // The campaign archive is skipped by name so its missions never reach
        // the skirmish chooser.
        if file_name.eq_ignore_ascii_case(MISSIONS_YRO_FILE_NAME) {
            continue;
        }
        let Some(pkt_name) = Path::new(&file_name)
            .with_extension("PKT")
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
        else {
            continue;
        };

        if let Some(manager) = assets.as_deref_mut() {
            if manager.register_loose_yro_archive(&path).is_err() {
                continue;
            }

            // Retail provenance: global YRO-derived PKT/map resolution —
            // `SessionClass::ScanMultiplayerMapFiles` @ `0x00699980`.
            let Some(pkt) = manager
                .get_ref(&pkt_name)
                .and_then(|bytes| IniFile::from_bytes(bytes).ok())
            else {
                continue;
            };
            append_pkt_records(
                records,
                &pkt,
                SkirmishScenarioSource::LooseYro(file_name),
                csf,
                PktTitleStyle::YroPlayerCountSuffix,
                |map_file| {
                    manager
                        .get_ref(map_file)
                        .and_then(|bytes| IniFile::from_bytes(bytes).ok())
                },
            );
        } else {
            let archive = match MixArchive::load(&path) {
                Ok(archive) => archive,
                Err(_) => continue,
            };
            let Some(pkt) = archive
                .get_by_name(&pkt_name)
                .and_then(|bytes| IniFile::from_bytes(bytes).ok())
            else {
                continue;
            };
            append_pkt_records(
                records,
                &pkt,
                SkirmishScenarioSource::LooseYro(file_name),
                csf,
                PktTitleStyle::YroPlayerCountSuffix,
                |map_file| {
                    archive
                        .get_by_name(map_file)
                        .and_then(|bytes| IniFile::from_bytes(bytes).ok())
                        .or_else(|| read_map_ini_for_metadata(&ra2_dir.join(map_file)))
                },
            );
        }
    }
    Ok(())
}

fn append_loose_yrm_records(
    records: &mut Vec<SkirmishScenarioRecord>,
    ra2_dir: &Path,
) -> Result<()> {
    for (path, file_name) in loose_files_with_extension(ra2_dir, "yrm")? {
        let Some(ini) = read_map_ini_for_metadata(&path) else {
            continue;
        };
        records.push(SkirmishScenarioRecord::concrete_from_ini(
            records.len(),
            SkirmishScenarioSource::LooseYrm(file_name),
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default(),
            &ini,
        ));
    }
    Ok(())
}

fn loose_files_with_extension(ra2_dir: &Path, extension: &str) -> Result<Vec<(PathBuf, String)>> {
    let mut files = Vec::new();

    for entry in std::fs::read_dir(&ra2_dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !retail_wildcard_file(&path) {
            continue;
        }
        let Some(file_name) = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        match ext.to_ascii_lowercase().as_str() {
            ext if ext.eq_ignore_ascii_case(extension) => files.push((path, file_name)),
            _ => {}
        }
    }
    Ok(files)
}

/// How a PKT-backed row's title is finished after the record is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PktTitleStyle {
    /// `MISSIONSMD.PKT` and loose `*.PKT` rows show the title verbatim.
    Verbatim,
    /// Only the `*.YRO` branch appends the entry's player-count span.
    YroPlayerCountSuffix,
}

/// Longest title the record's title slot holds, in characters.
///
/// The native slot is 0x2C wide chars, but after the copy-and-append the
/// terminator is forced into the last one, so the visible title caps one short
/// of the slot. An over-long custom title is cut, not wrapped.
const PKT_TITLE_MAX_CHARS: usize = 0x2C - 1;

/// Native formats the span as `(n)` when min equals max, else `(min-max)`, and
/// joins it to the title with a single space.
fn yro_player_count_suffix(min_players: u8, max_players: u8) -> String {
    if min_players == max_players {
        format!(" ({min_players})")
    } else {
        format!(" ({min_players}-{max_players})")
    }
}

fn truncate_pkt_title(title: &str) -> String {
    title.chars().take(PKT_TITLE_MAX_CHARS).collect()
}

fn append_pkt_records<F>(
    records: &mut Vec<SkirmishScenarioRecord>,
    pkt: &IniFile,
    source: SkirmishScenarioSource,
    csf: Option<&CsfFile>,
    title_style: PktTitleStyle,
    mut map_ini: F,
) where
    F: FnMut(&str) -> Option<IniFile>,
{
    let Some(multimaps) = pkt.section("MultiMaps") else {
        return;
    };
    for map_stem in multimaps.get_values() {
        let map_stem = map_stem.trim();
        if map_stem.is_empty() {
            continue;
        }
        let file_name = format!("{map_stem}.MAP");
        let Some(map_ini) = map_ini(&file_name) else {
            continue;
        };
        let entry_section = pkt.section(map_stem);
        let display_name = pkt_display_name(pkt, map_stem, csf)
            .unwrap_or_else(|| display_name_from_basic_or_file(&map_ini, &file_name));
        let fields = PktEntryFields {
            display_name,
            // The mode filter, like the title and the player bounds, is a
            // property of the PKT entry; the map payload never carries it.
            game_modes: entry_section
                .and_then(|section| section.get("GameMode"))
                .map(parse_game_mode_list)
                .unwrap_or_default(),
            min_players: entry_section
                .and_then(|section| section.get_i32("MinPlayers"))
                .and_then(pkt_player_count),
            max_players: entry_section
                .and_then(|section| section.get_i32("MaxPlayers"))
                .and_then(pkt_player_count),
        };
        let mut record = SkirmishScenarioRecord::pkt_from_ini(
            records.len(),
            source.clone(),
            &file_name,
            &map_ini,
            fields,
        );
        if title_style == PktTitleStyle::YroPlayerCountSuffix {
            let (Some(min), Some(max)) = (record.min_players, record.max_players) else {
                records.push(record);
                continue;
            };
            record.display_name = truncate_pkt_title(&format!(
                "{}{}",
                record.display_name,
                yro_player_count_suffix(min, max)
            ));
        }
        records.push(record);
    }
}

fn pkt_player_count(value: i32) -> Option<u8> {
    u8::try_from(value).ok()
}

fn pkt_display_name(pkt: &IniFile, map_stem: &str, csf: Option<&CsfFile>) -> Option<String> {
    let section = pkt.section(map_stem)?;
    if let Some(value) = section
        .get("DescriptionText")
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(value.to_string());
    }

    section
        .get("Description")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            csf.map(|csf| csf.text(value).into_owned())
                .unwrap_or_else(|| value.to_string())
        })
}

fn display_name_from_basic_or_file(ini: &IniFile, file_name: &str) -> String {
    crate::map::basic::parse_basic_section(ini)
        .name
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| file_name.to_string())
}

fn read_ini_file(path: &Path) -> Option<IniFile> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| IniFile::from_bytes(&bytes).ok())
}

pub(crate) fn read_map_menu_entry(path: &Path, file_name: &str) -> MapMenuEntry {
    let fallback = || MapMenuEntry {
        file_name: file_name.to_string(),
        display_name: file_name.to_string(),
        author: None,
        briefing: BriefingSection::default(),
        preview: PreviewSection::default(),
        multiplayer_start_waypoints: Vec::new(),
        player_capacity: DEFAULT_SKIRMISH_PLAYER_CAPACITY,
        preview_source_bounds: None,
    };

    let ini = match read_map_ini_for_metadata(path) {
        Some(ini) => ini,
        None => return fallback(),
    };

    read_map_menu_entry_from_ini(&ini, file_name)
}

// The INI -> entry parser is map-owned (F06 critic fix); re-exported here for
// the app map-listing callers.
pub(crate) use crate::map::scenario_menu::read_map_menu_entry_from_ini;


pub(crate) fn read_map_ini_for_metadata(path: &Path) -> Option<IniFile> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() >= 2 && bytes[0] == 0 && bytes[1] == 0 {
        // MIX-wrapped: pick the entry that parses as INI with a [Map] section
        // (retail map MIXes also contain a tiny [MultiMaps] description stub).
        let archive = MixArchive::load(path).ok()?;
        let mut entries = archive.entries().to_vec();
        entries.sort_by(|a, b| b.size.cmp(&a.size));
        for entry in &entries {
            let data = archive.get_by_id(entry.id)?;
            if let Ok(ini) = IniFile::from_bytes(data) {
                if ini.section("Map").is_some() {
                    return Some(ini);
                }
            }
        }
        None
    } else {
        IniFile::from_bytes(&bytes).ok()
    }
}

pub(crate) fn load_map_by_name_or_path(ra2_dir: &Path, map_name: &str) -> Result<LoadedMap> {
    let direct: PathBuf = PathBuf::from(map_name);
    if direct.is_absolute() && direct.exists() {
        return load_map_from_path_with_source(&direct);
    }

    let in_ra2: PathBuf = ra2_dir.join(map_name);
    if in_ra2.exists() {
        return load_map_from_path_with_source(&in_ra2);
    }

    for ext in ["mmx", "yro", "map", "mpr", "yrm"] {
        let candidate = ra2_dir.join(format!("{}.{}", map_name, ext));
        if candidate.exists() {
            return load_map_from_path_with_source(&candidate);
        }
    }

    Err(anyhow::anyhow!(
        "Map '{}' not found (checked the retail root and .mmx/.yro/.map/.mpr/.yrm variants)",
        map_name
    ))
}

fn retail_wildcard_file(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x116 == 0
    }
    #[cfg(not(windows))]
    {
        true
    }
}

pub(crate) fn load_map_by_name_or_path_with_assets(
    ra2_dir: &Path,
    map_name: &str,
    assets: &AssetManager,
) -> Result<LoadedMap> {
    match load_map_by_name_or_path(ra2_dir, map_name) {
        Ok(map) => return Ok(map),
        Err(local_err) => {
            for candidate in asset_map_candidates(map_name) {
                if let Some(resolved) = assets.resolve_ref(&candidate) {
                    log::info!("Loaded map {candidate} from MIX assets");
                    let map = MapFile::from_bytes(resolved.bytes)?;
                    return Ok(LoadedMap {
                        map,
                        source: LoadedMapSource::Mix {
                            logical_name: candidate,
                            source_archive: resolved.source_archive.to_string(),
                            entry_id: resolved.entry_id,
                            payload_len: resolved.bytes.len(),
                        },
                    });
                }
            }
            Err(local_err)
        }
    }
}

pub(crate) fn asset_map_candidates(map_name: &str) -> Vec<String> {
    let mut names = Vec::new();
    names.push(map_name.to_string());
    let has_extension = Path::new(map_name).extension().is_some();
    if !has_extension {
        for ext in ["mmx", "yro", "map", "mpr", "yrm"] {
            names.push(format!("{map_name}.{ext}"));
        }
    }
    names
}

pub(crate) fn load_map_from_path(path: &Path) -> Result<MapFile> {
    map_file::load_from_path(path).map_err(Into::into)
}

fn load_map_from_path_with_source(path: &Path) -> Result<LoadedMap> {
    let payload_len = std::fs::metadata(path)?.len() as usize;
    let map = load_map_from_path(path)?;
    Ok(LoadedMap {
        map,
        source: LoadedMapSource::Loose {
            path: path.to_path_buf(),
            payload_len,
        },
    })
}

/// Try loading .mmx map files from a list of candidates.
pub(crate) fn try_load_mmx(ra2_dir: &Path, names: &[&str]) -> Result<LoadedMap> {
    for &name in names {
        let path: PathBuf = ra2_dir.join(name);
        if path.exists() {
            match map_file::load_mmx(&path) {
                Ok(mf) => {
                    log::info!("Loaded map from {}", name);
                    let payload_len = std::fs::metadata(&path)?.len() as usize;
                    return Ok(LoadedMap {
                        map: mf,
                        source: LoadedMapSource::Loose { path, payload_len },
                    });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::preview::{PreviewSourceBounds, PreviewStartPoint};
    use crate::assets::mix_hash::mix_hash;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "vera20k-app-list-maps-{label}-{}-{sequence}",
                std::process::id()
            ));
            std::fs::create_dir(&path).expect("create test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn write(&self, name: &str, bytes: &[u8]) {
            std::fs::write(self.0.join(name), bytes).expect("write test file");
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn make_new_format_mix_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
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

    #[test]
    fn menu_entry_exposes_sorted_multiplayer_start_waypoints() {
        let ini = IniFile::from_str(
            "[Basic]\nName=Waypoint Test\nNewINIFormat=5\n[Waypoints]\n7=120034\n0=100011\n99=55098\n3=110022\n",
        );
        let entry = read_map_menu_entry_from_ini(&ini, "test.map");
        let indices: Vec<u32> = entry
            .multiplayer_start_waypoints
            .iter()
            .map(|wp| wp.index)
            .collect();
        assert_eq!(indices, vec![0, 3, 7]);
        assert_eq!(entry.multiplayer_start_waypoints[0].rx, 11);
        assert_eq!(entry.multiplayer_start_waypoints[0].ry, 100);
        assert_eq!(entry.player_capacity, 3);
        assert_eq!(entry.preview_source_bounds, None);
    }

    #[test]
    fn menu_entry_carries_random_map_capacity_fallback() {
        let ini = IniFile::from_str("[RandomMap]\nNumPlayers=4\n");
        let entry = read_map_menu_entry_from_ini(&ini, "RandMap.Sed");

        assert!(entry.multiplayer_start_waypoints.is_empty());
        assert_eq!(entry.player_capacity, 4);
    }

    #[test]
    fn menu_entry_exposes_header_preview_start_bounds() {
        let ini = IniFile::from_str(
            "[Basic]\nName=Header Starts\nNewINIFormat=5\n\
             [Header]\nStartX=10\nStartY=20\nWidth=100\nHeight=80\n\
             NumberStartingPoints=2\nWaypoint1=25,30\nWaypoint2=90,70\n",
        );
        let entry = read_map_menu_entry_from_ini(&ini, "test.map");
        assert_eq!(
            entry.preview_source_bounds,
            Some(PreviewSourceBounds {
                origin_x: 10,
                origin_y: 20,
                width: 100,
                height: 80,
                start_points: vec![
                    PreviewStartPoint { x: 25, y: 30 },
                    PreviewStartPoint { x: 90, y: 70 },
                ],
            })
        );
    }

    #[test]
    fn invalid_header_start_count_disables_live_preview_markers() {
        let ini = IniFile::from_str(
            "[Header]\nStartX=0\nStartY=0\nWidth=100\nHeight=80\nNumberStartingPoints=9\n",
        );
        let entry = read_map_menu_entry_from_ini(&ini, "test.map");
        assert_eq!(entry.preview_source_bounds, None);
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

    #[test]
    fn map_source_manifest_preserves_actual_mix_lookup_facts() {
        let source = LoadedMapSource::Mix {
            logical_name: "Fight.MAP".to_string(),
            source_archive: "multimd.mix".to_string(),
            entry_id: 0x9306_F050_u32 as i32,
            payload_len: 91_254,
        };
        let json = serde_json::to_value(source).expect("serialize source");

        assert_eq!(json["kind"], "mix");
        assert_eq!(json["logical_name"], "Fight.MAP");
        assert_eq!(json["source_archive"], "multimd.mix");
        assert_eq!(json["entry_id"], 0x9306_F050_u32 as i32);
        assert_eq!(json["payload_len"], 91_254);
    }

    fn encode_csf_string(s: &str) -> Vec<u8> {
        s.encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .map(|b| !b)
            .collect()
    }

    fn build_test_csf(entries: &[(&str, &str)]) -> CsfFile {
        let mut data = Vec::new();
        data.extend_from_slice(&0x4353_4620u32.to_le_bytes());
        data.extend_from_slice(&3u32.to_le_bytes());
        data.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        data.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&[0u8; 6]);

        for (label, value) in entries {
            let encoded_value = encode_csf_string(value);
            data.extend_from_slice(&0x4C42_4C20u32.to_le_bytes());
            data.extend_from_slice(&1u32.to_le_bytes());
            data.extend_from_slice(&(label.len() as u32).to_le_bytes());
            data.extend_from_slice(label.as_bytes());
            data.extend_from_slice(&0x5354_5220u32.to_le_bytes());
            data.extend_from_slice(&(value.encode_utf16().count() as u32).to_le_bytes());
            data.extend_from_slice(&encoded_value);
        }

        CsfFile::from_bytes(&data).expect("test CSF should parse")
    }

    #[test]
    fn pkt_records_preserve_multimaps_source_order_and_pkt_names() {
        let pkt = IniFile::from_str(
            "[MultiMaps]\n1=Zoo\n2=Alpha\n3=Raw\n\
             [Zoo]\nDescriptionText=Zoo Display\n\
             [Alpha]\nDescription=GUI:AlphaName\n",
        );
        let csf = build_test_csf(&[("GUI:AlphaName", "Localized Alpha")]);

        let mut records = Vec::new();
        append_pkt_records(
            &mut records,
            &pkt,
            SkirmishScenarioSource::MissionsMdPkt,
            Some(&csf),
            PktTitleStyle::Verbatim,
            |file_name| match file_name {
                "Zoo.MAP" => Some(IniFile::from_str("[Basic]\nName=Basic Zoo\n")),
                "Alpha.MAP" => Some(IniFile::from_str("[Basic]\nName=Basic Alpha\n")),
                "Raw.MAP" => Some(IniFile::from_str("[Basic]\nName=Basic Raw\n")),
                _ => None,
            },
        );

        let names: Vec<&str> = records
            .iter()
            .map(|record| record.display_name.as_str())
            .collect();
        assert_eq!(names, vec!["Zoo Display", "Localized Alpha", "Basic Raw"]);
        assert_eq!(records[0].file_name, "Zoo.MAP");
        assert_eq!(records[1].file_name, "Alpha.MAP");
        assert_eq!(records[2].file_name, "Raw.MAP");
    }

    /// Shaped after the real `MISSIONSMD.PKT`: the entry section carries the
    /// filter list and bounds, the map payload carries neither.
    fn retail_shaped_pkt() -> IniFile {
        IniFile::from_str(
            "[MultiMaps]\n1=XMP02T2\n2=XMP21\n\
             [XMP02T2]\nDescriptionText=Dustbowl\nCD=2\nMinPlayers=2\nMaxPlayers=2\n\
             GameMode=standard, meatgrind\n\
             [XMP21]\nDescriptionText=Circuit Board\nMinPlayers=2\nMaxPlayers=4\n\
             GameMode=teamgame\n",
        )
    }

    fn bare_map_payload(_file_name: &str) -> Option<IniFile> {
        Some(IniFile::from_str("[Basic]\nOfficial=yes\n"))
    }

    #[test]
    fn pkt_rows_carry_the_entry_game_mode_list_so_non_standard_modes_list_maps() {
        let mut records = Vec::new();
        append_pkt_records(
            &mut records,
            &retail_shaped_pkt(),
            SkirmishScenarioSource::MissionsMdPkt,
            None,
            PktTitleStyle::Verbatim,
            bare_map_payload,
        );

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].game_modes, vec!["standard", "meatgrind"]);
        assert_eq!(records[0].min_players, Some(2));
        assert_eq!(records[0].max_players, Some(2));
        assert_eq!(records[1].game_modes, vec!["teamgame"]);
        assert_eq!(records[1].max_players, Some(4));
    }

    #[test]
    fn yro_rows_append_the_entry_player_count_span_to_the_title() {
        let mut records = Vec::new();
        append_pkt_records(
            &mut records,
            &retail_shaped_pkt(),
            SkirmishScenarioSource::LooseYro("CrctBrd.yro".to_string()),
            None,
            PktTitleStyle::YroPlayerCountSuffix,
            bare_map_payload,
        );

        // Equal bounds print one number; differing bounds print the span.
        assert_eq!(records[0].display_name, "Dustbowl (2)");
        assert_eq!(records[1].display_name, "Circuit Board (2-4)");
    }

    #[test]
    fn pkt_and_missions_pkt_rows_keep_their_title_verbatim() {
        let mut records = Vec::new();
        append_pkt_records(
            &mut records,
            &retail_shaped_pkt(),
            SkirmishScenarioSource::MissionsMdPkt,
            None,
            PktTitleStyle::Verbatim,
            bare_map_payload,
        );

        assert_eq!(records[0].display_name, "Dustbowl");
        assert_eq!(records[1].display_name, "Circuit Board");
    }

    #[test]
    fn yro_title_suffix_is_bounded_to_the_native_title_slot() {
        // 0x2C wide slots less the forced terminator.
        assert_eq!(PKT_TITLE_MAX_CHARS, 43);
        let long = "M".repeat(PKT_TITLE_MAX_CHARS + 8);
        let pkt = IniFile::from_str(&format!(
            "[MultiMaps]\n1=Long\n[Long]\nDescriptionText={long}\nMinPlayers=2\nMaxPlayers=4\n"
        ));
        let mut records = Vec::new();
        append_pkt_records(
            &mut records,
            &pkt,
            SkirmishScenarioSource::LooseYro("long.yro".to_string()),
            None,
            PktTitleStyle::YroPlayerCountSuffix,
            bare_map_payload,
        );

        assert_eq!(records[0].display_name.chars().count(), 43);
    }

    #[test]
    fn yro_discovery_uses_global_loose_winners_and_retains_registered_archive() {
        let directory = TestDirectory::new("yro-global-collisions");
        let embedded_pkt = b"[MultiMaps]\n1=Arena\n[Arena]\nDescriptionText=Embedded PKT\nMinPlayers=2\nMaxPlayers=2\n";
        let embedded_map = b"[Basic]\nName=Embedded Map\nAuthor=Embedded Author\n";
        let yro = make_new_format_mix_bytes(&[
            ("COLLIDE.PKT", &embedded_pkt[..]),
            ("Arena.MAP", &embedded_map[..]),
            ("YROONLY.BIN", &b"retained"[..]),
        ]);
        directory.write("COLLIDE.YRO", &yro);
        directory.write(
            "COLLIDE.PKT",
            b"[MultiMaps]\n1=Arena\n[Arena]\nDescriptionText=Loose PKT\nMinPlayers=2\nMaxPlayers=4\n",
        );
        directory.write(
            "Arena.MAP",
            b"[Basic]\nName=Loose Map\nAuthor=Loose Author\n",
        );

        let mut manager = AssetManager::from_loose_root_for_test(directory.path());
        let mut records = Vec::new();
        append_loose_yro_records(&mut records, directory.path(), Some(&mut manager), None)
            .expect("scan loose YRO");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].file_name, "Arena.MAP");
        assert_eq!(records[0].display_name, "Loose PKT (2-4)");
        assert_eq!(records[0].author.as_deref(), Some("Loose Author"));
        assert_eq!(
            records[0].source,
            SkirmishScenarioSource::LooseYro("COLLIDE.YRO".to_string())
        );
        assert_eq!(manager.registered_archive_names(), ["COLLIDE.YRO"]);
        assert_eq!(manager.get_ref("YROONLY.BIN"), Some(&b"retained"[..]));
    }

    #[test]
    fn loose_scan_exclusion_names_match_the_native_wildcard_skips() {
        // Both are compared case-insensitively against the found file name.
        assert!("missionsmd.pkt".eq_ignore_ascii_case(MISSIONS_MD_PKT_FILE_NAME));
        assert!("Missions.Yro".eq_ignore_ascii_case(MISSIONS_YRO_FILE_NAME));
        assert!(!"MISSIONS.PKT".eq_ignore_ascii_case(MISSIONS_MD_PKT_FILE_NAME));
    }
}
