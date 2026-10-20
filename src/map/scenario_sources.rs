//! Retail scenario discovery, retained archive registration and chooser metadata.
//!
//! Map source resolution and consumed-byte identity are owned by `map::source`.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::assets::asset_manager::{AssetManager, retail_enumerated_file};
use crate::assets::csf_file::CsfFile;
use crate::map::preview::PreviewSection;
use crate::map::scenario_menu::MapMenuEntry;
use crate::map::scenario_menu::read_map_menu_entry_from_ini;
use crate::map::skirmish_scenarios::{
    PKT_DEFAULT_MAX_PLAYERS, PKT_DEFAULT_MIN_PLAYERS, PKT_GAME_MODE_CAPACITY, PktEntryFields,
    SkirmishScenarioRecord, SkirmishScenarioSource, read_game_modes,
};
use crate::map::source::read_map_ini_for_metadata;
use crate::map::waypoints::DEFAULT_SKIRMISH_PLAYER_CAPACITY;
use crate::rules::ini_parser::IniFile;
use crate::util::config::GameConfig;

/// Names the loose wildcard scans skip. Both are exclusions, not prerequisites:
/// the archived `MISSIONSMD.PKT` is already consumed as the first source, and
/// `MISSIONS.YRO` holds campaign missions that never belong in the multiplayer
/// chooser.
const MISSIONS_MD_PKT_FILE_NAME: &str = "MISSIONSMD.PKT";
const MISSIONS_YRO_FILE_NAME: &str = "MISSIONS.YRO";

/// List the maps in the RA2 directory: the Skirmish scenario records' fallback
/// when the asset-backed scan finds none.
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
        if !retail_enumerated_file(&path) {
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let ext = ext.to_ascii_lowercase();
        if matches!(ext.as_str(), "mmx" | "yro" | "map" | "mpr" | "yrm")
            && let Some(name) = path.file_name().and_then(|n| n.to_str())
        {
            maps.push(read_map_menu_entry(&path, name));
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
pub fn list_skirmish_scenario_records_with_assets(
    ra2_dir: &Path,
    assets: &mut AssetManager,
    csf: Option<&CsfFile>,
) -> Result<Vec<SkirmishScenarioRecord>> {
    let mut records = Vec::new();

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

    append_loose_pkt_records(&mut records, ra2_dir, assets, csf)?;
    append_loose_yro_records(&mut records, ra2_dir, assets, csf)?;
    append_loose_yrm_records(&mut records, ra2_dir)?;

    Ok(records)
}

fn append_loose_pkt_records(
    records: &mut Vec<SkirmishScenarioRecord>,
    ra2_dir: &Path,
    assets: &AssetManager,
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
                        .get_ref(map_file)
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
    manager: &mut AssetManager,
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

    for entry in std::fs::read_dir(ra2_dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !retail_enumerated_file(&path) {
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
    // `ScanMultiplayerMapFiles` 0x00699980: each entry through ReadString(0x40).
    for key in multimaps.keys() {
        let Some(map_stem) = multimaps.read_name(key, 0x40) else {
            continue;
        };
        let file_name = format!("{map_stem}.MAP");
        let Some(map_ini) = map_ini(&file_name) else {
            continue;
        };
        let entry_section = pkt.section_or_empty(map_stem);
        let display_name = pkt_display_name(pkt, map_stem, csf)
            .unwrap_or_else(|| display_name_from_basic_or_file(&map_ini, &file_name));
        let fields = PktEntryFields {
            display_name,
            // The mode filter, like the title and the player bounds, is a
            // property of the PKT entry; the map payload never carries it.
            game_modes: read_game_modes(entry_section, PKT_GAME_MODE_CAPACITY),
            min_players: pkt_player_count(
                entry_section.read_int("MinPlayers", PKT_DEFAULT_MIN_PLAYERS.into()),
            ),
            max_players: pkt_player_count(
                entry_section.read_int("MaxPlayers", PKT_DEFAULT_MAX_PLAYERS.into()),
            ),
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

/// `MPGameFileEntry__Constructor`: `DescriptionText` is read into 0x2C bytes;
/// otherwise `Description` is a string-table label (`0x00529160`, 0x3FF
/// bytes). Native branches on `DescriptionText` being present and fetches
/// even an empty label, and caps the fetched text at 0x2B units; Rust falls
/// through to the next source instead.
fn pkt_display_name(pkt: &IniFile, map_stem: &str, csf: Option<&CsfFile>) -> Option<String> {
    let section = pkt.section(map_stem)?;
    if let Some(value) = section.read_name("DescriptionText", 0x2C) {
        return Some(value.to_string());
    }
    section.read_name("Description", 0x400).map(|label| {
        csf.map(|csf| csf.text(label).into_owned())
            .unwrap_or_else(|| label.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::preview::{PreviewSourceBounds, PreviewStartPoint};
    use crate::map::source::test_support::{TestDirectory, make_new_format_mix_bytes};

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
        append_loose_yro_records(&mut records, directory.path(), &mut manager, None)
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
}
