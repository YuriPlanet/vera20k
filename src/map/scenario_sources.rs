//! Retail scenario discovery, retained archive registration and chooser metadata.
//!
//! Map source resolution and consumed-byte identity are owned by `map::source`.

use std::collections::{HashMap, HashSet};
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
use crate::map::source::{
    LoadedMapSource, game_folder_file_in, game_folders_in, read_map_ini_for_metadata,
};
use crate::map::waypoints::DEFAULT_SKIRMISH_PLAYER_CAPACITY;
use crate::rules::ini_parser::IniFile;
use crate::util::case_insensitive_path::{native_listing, native_listing_over};
use crate::util::config::GameConfig;
use crate::util::native_string::decode_widened_utf8;

/// Names the loose wildcard scans skip. Both are exclusions, not prerequisites:
/// the archived `MISSIONSMD.PKT` is already consumed as the first source, and
/// `MISSIONS.YRO` holds campaign missions that never belong in the multiplayer
/// chooser.
const MISSIONS_MD_PKT_FILE_NAME: &str = "MISSIONSMD.PKT";
const MISSIONS_YRO_FILE_NAME: &str = "MISSIONS.YRO";

/// List the maps in the RA2 directory: the Skirmish scenario records' fallback
/// when the asset-backed scan finds none.
///
/// Reads metadata from the five supported map extensions. Loose `.mpr` files
/// follow the same multiplayer eligibility rules as the main catalog.
pub fn list_available_maps(assets: Option<&AssetManager>) -> Result<Vec<MapMenuEntry>> {
    let config: GameConfig = GameConfig::load()?;
    list_available_maps_in(&config.paths.ra2_dir, assets)
}

fn list_available_maps_in(
    ra2_dir: &Path,
    assets: Option<&AssetManager>,
) -> Result<Vec<MapMenuEntry>> {
    let mut maps: Vec<MapMenuEntry> = Vec::new();
    for entry in crate::util::file_system::read_dir(ra2_dir)? {
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
            if ext == "mpr" {
                if let Some(record) = loose_map_record(&path, name.into(), maps.len(), assets) {
                    maps.push(record.to_map_menu_entry());
                }
            } else {
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
pub fn list_skirmish_scenario_records_with_assets(
    ra2_dir: &Path,
    assets: &mut AssetManager,
    csf: Option<&CsfFile>,
) -> Result<Vec<SkirmishScenarioRecord>> {
    skirmish_scenario_records_in(
        ra2_dir,
        assets,
        csf,
        crate::map::source::downloaded_maps_dir(),
    )
}

fn skirmish_scenario_records_in(
    ra2_dir: &Path,
    assets: &mut AssetManager,
    csf: Option<&CsfFile>,
    maps_dir: Option<&Path>,
) -> Result<Vec<SkirmishScenarioRecord>> {
    let mut records = Vec::new();

    if let (Some(client), Some(root)) = (assets.client_install(), assets.mod_dir()) {
        // A client catalog owns this folder's scenarios. The retail PKT is
        // unrelated and can list maps with the wrong rules for this game.
        let modes = crate::skirmish_modes::skirmish_modes_from_assets(assets)?;
        for map in &client.maps {
            if !modes.iter().any(|mode| {
                map.game_modes
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&mode.map_filter))
            }) {
                continue;
            }
            let Some(ini) = read_map_ini_for_metadata(&root.join(&map.file_name)) else {
                continue;
            };
            let mut record = SkirmishScenarioRecord::concrete_from_ini(
                records.len(),
                SkirmishScenarioSource::ClientCatalog,
                &map.file_name,
                &ini,
            );
            if let Some(name) = &map.display_name {
                record.display_name.clone_from(name);
            }
            record.game_modes.clone_from(&map.game_modes);
            record.min_players = map.min_players;
            record.max_players = map.max_players;
            if let Some(max_players) = map.max_players {
                record.player_capacity = record.player_capacity.min(i32::from(max_players));
            }
            records.push(record);
        }
        return Ok(records);
    }

    if let Some(pkt) = assets
        .get_ref(assets.ini_roots().map_catalog())
        .and_then(|bytes| IniFile::from_bytes(bytes).ok())
    {
        append_pkt_records(
            &mut records,
            &pkt,
            SkirmishScenarioSource::MissionsMdPkt,
            csf,
            |file_name| {
                assets
                    .get_ref(file_name)
                    .and_then(|bytes| IniFile::from_bytes(bytes).ok())
            },
        );
    }

    append_loose_pkt_records(&mut records, ra2_dir, assets, csf)?;
    // `.yro` (official YR skirmish maps) is a Yuri's Revenge format. An
    // original-RA2 launch must not surface it in the Skirmish chooser — RA2
    // maps come from MAPS01/02 and missions.pkt. Loose-map scanning also reads
    // RA2's own `.mpr` maps, so it stays available in both modes.
    if !crate::util::ra2_launch::enabled() {
        append_loose_yro_records(&mut records, ra2_dir, assets, csf)?;
    }
    append_loose_map_records(
        &mut records,
        &game_folders_in(ra2_dir, assets.mod_dir()),
        maps_dir,
        assets,
    )?;

    // The stock YR catalog also looks up its release maps by name. The loose
    // scan above already handles files installed in the player's Maps folder.
    if assets.mod_dir().is_none()
        && assets.ini_roots().is_stock()
        && !crate::util::ra2_launch::enabled()
    {
        // A fresh profile contains Arctic Circle before the first scan. If it
        // is the only loose row, keep it for the release-map pass so the
        // game-folder fallback still lists maps from uncatalogued installs.
        let mut installed_arctic = match records.as_slice() {
            [row]
                if row
                    .file_name
                    .eq_ignore_ascii_case(crate::map::installed::ARCTIC_MAP)
                    && matches!(row.source, SkirmishScenarioSource::LooseMap(_)) =>
            {
                records.pop()
            }
            _ => None,
        };
        if records.is_empty() {
            // Keep the normal shell fallback for installs without a PKT/YRO
            // catalog, before Ice Lake makes this list nonempty.
            records.extend(
                list_available_maps_in(ra2_dir, Some(assets))?
                    .iter()
                    .enumerate()
                    .map(|(index, map)| SkirmishScenarioRecord::from_map_menu_entry(index, map)),
            );
        }
        for name in [
            crate::map::source::ICE_LAKE,
            crate::map::installed::ARCTIC_MAP,
        ] {
            if records
                .iter()
                .any(|record| record.file_name.eq_ignore_ascii_case(name))
            {
                continue;
            }
            if let Some(mut row) =
                installed_arctic.take_if(|row| row.file_name.eq_ignore_ascii_case(name))
            {
                row.source_ordinal = records.len();
                records.push(row);
                continue;
            }
            let Ok(loaded) =
                crate::map::source::load_map_ini_by_name_or_path_with_assets(ra2_dir, name, assets)
            else {
                continue;
            };
            // A same-name file in the YR root or in a MIX may have no PKT
            // row. Describe and preview the physical map the match will use.
            let source = match loaded.source {
                LoadedMapSource::Loose { .. } => SkirmishScenarioSource::LooseMap(name.to_owned()),
                LoadedMapSource::Mix { .. } => SkirmishScenarioSource::ArchiveMap,
                LoadedMapSource::Bundled { .. } => SkirmishScenarioSource::Bundled,
                LoadedMapSource::Generated { .. } | LoadedMapSource::LegacyFallback { .. } => {
                    continue;
                }
            };
            records.push(SkirmishScenarioRecord::concrete_from_ini(
                records.len(),
                source,
                name,
                &loaded.ini,
            ));
        }
    }

    Ok(records)
}

fn append_loose_pkt_records(
    records: &mut Vec<SkirmishScenarioRecord>,
    ra2_dir: &Path,
    assets: &AssetManager,
    csf: Option<&CsfFile>,
) -> Result<()> {
    let paths = native_listing_over(&game_folders_in(ra2_dir, assets.mod_dir()))?;
    for (path, file_name) in loose_files_with_extensions(paths, &["pkt"]) {
        // The wildcard scan would otherwise re-list every archived row a second
        // time from an extracted copy sitting beside the executable.
        if file_name.eq_ignore_ascii_case(MISSIONS_MD_PKT_FILE_NAME)
            || file_name.eq_ignore_ascii_case(assets.ini_roots().map_catalog())
        {
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
            |map_file| {
                read_map_ini_for_metadata(&game_folder_file_in(ra2_dir, assets.mod_dir(), map_file))
                    .or_else(|| {
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
    let paths = native_listing_over(&game_folders_in(ra2_dir, manager.mod_dir()))?;
    for (path, file_name) in loose_files_with_extensions(paths, &["yro"]) {
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
            |map_file| {
                manager
                    .get_ref(map_file)
                    .and_then(|bytes| IniFile::from_bytes(bytes).ok())
            },
        );
    }
    Ok(())
}

fn append_loose_map_records(
    records: &mut Vec<SkirmishScenarioRecord>,
    game_folders: &[&Path],
    maps_folder: Option<&Path>,
    assets: &AssetManager,
) -> Result<()> {
    let installed = native_listing_over(game_folders)?;
    // Lookup tries each name literally, then with a map suffix appended, in
    // the mod folder before the game folder and both before the user's Maps.
    // A lower folder's row must never describe a different file than it loads.
    // None is the user's Maps folder, below the game and selected mod.
    let folder_rank = |path: &Path| {
        game_folders
            .iter()
            .position(|dir| Some(*dir) == path.parent())
    };
    let mut installed_names = HashMap::new();
    for path in installed
        .iter()
        .filter(|path| crate::map::source::is_loose_map_name(path))
    {
        let rank = folder_rank(path);
        for name in [path.file_name(), path.file_stem()].into_iter().flatten() {
            if let Some(name) = name.to_str() {
                let highest = installed_names
                    .entry(name.to_ascii_lowercase())
                    .or_insert(rank);
                *highest = (*highest).max(rank);
            }
        }
    }
    // Retail ScanMultiplayerMapFiles (0x00699980) lists *.YRM. VERA also
    // allows the original Red Alert 2 multiplayer extension in these folders.
    let mut files = loose_files_with_extensions(installed, &["yrm", "mpr"]);
    if let Some(downloaded) = maps_folder
        && crate::util::file_system::exists(downloaded)
    {
        let paths = native_listing(downloaded).unwrap_or_else(|error| {
            log::warn!("Could not list {}: {error}", downloaded.display());
            Vec::new()
        });
        files.extend(loose_files_with_extensions(
            paths,
            crate::map::source::CUSTOM_MAP_EXTENSIONS,
        ));
    }
    let mut listed: HashSet<_> = records
        .iter()
        .map(|record| record.file_name.to_ascii_lowercase())
        .collect();
    for (path, file_name) in files {
        let key = file_name.to_ascii_lowercase();
        if installed_names
            .get(&key)
            .is_some_and(|&rank| rank > folder_rank(&path))
            || !listed.insert(key)
        {
            continue;
        }
        if let Some(record) = loose_map_record(&path, file_name, records.len(), Some(assets)) {
            records.push(record);
        }
    }
    Ok(())
}

/// The Choose Map row of a loose multiplayer map, from its own `[Basic]`;
/// `None` when it does not read as a map.
pub fn loose_map_record(
    path: &Path,
    file_name: String,
    ordinal: usize,
    assets: Option<&AssetManager>,
) -> Option<SkirmishScenarioRecord> {
    let mpr = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("mpr"));
    let ini = match assets.filter(|_| mpr) {
        Some(assets) => crate::map::source::read_effective_loose_map_ini(path, assets)?,
        None => read_map_ini_for_metadata(path)?,
    };
    if mpr {
        let basic = ini.section("Basic")?;
        if ini.section("Map").is_none() || !basic.read_bool("MultiplayerOnly", true) {
            return None;
        }
    }
    Some(SkirmishScenarioRecord::concrete_from_ini(
        ordinal,
        SkirmishScenarioSource::LooseMap(file_name),
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default(),
        &ini,
    ))
}

/// Keep eligible map files in native enumeration order.
fn loose_files_with_extensions(paths: Vec<PathBuf>, extensions: &[&str]) -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();

    // `SessionClass::ScanMultiplayerMapFiles` (`0x00699980`) lists with
    // FindFirstFileA (`0x00699AE6`, `0x00699C5D`, `0x0069A007`) and registers
    // each YRO as it is listed.
    for path in paths {
        if !retail_enumerated_file(&path) {
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if extensions
            .iter()
            .any(|known| ext.eq_ignore_ascii_case(known))
            && let Some(name) = path.file_name().and_then(|name| name.to_str())
        {
            let name = name.to_owned();
            files.push((path, name));
        }
    }
    files
}

/// How a PKT-backed row's title is finished after the record is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PktTitleStyle {
    /// `MISSIONSMD.PKT` and loose `*.PKT` rows show the title verbatim.
    Verbatim,
    /// Only the `*.YRO` branch appends the entry's player-count span.
    YroPlayerCountSuffix,
}

impl PktTitleStyle {
    fn of(source: &SkirmishScenarioSource) -> Self {
        match source {
            SkirmishScenarioSource::LooseYro(_) => Self::YroPlayerCountSuffix,
            _ => Self::Verbatim,
        }
    }
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

/// Sets `record`'s title from `title`, its PKT entry's own text, finished in
/// the style of the record's source.
fn finish_pkt_title(record: &mut SkirmishScenarioRecord, title: String) {
    let style = PktTitleStyle::of(&record.source);
    record.display_name = match (style, record.min_players, record.max_players) {
        (PktTitleStyle::YroPlayerCountSuffix, Some(min), Some(max)) => {
            truncate_pkt_title(&format!("{title}{}", yro_player_count_suffix(min, max)))
        }
        _ => title,
    };
}

/// VERA: reads each title the string table names again from `csf`. The scan
/// read them in the language then in use, and a language picked since gives
/// the table other texts.
pub fn relabel_records(records: &mut [SkirmishScenarioRecord], csf: &CsfFile) {
    for record in records {
        if let Some(label) = record.title_label.clone() {
            finish_pkt_title(record, csf.text(&label).into_owned());
        }
    }
}

fn append_pkt_records<F>(
    records: &mut Vec<SkirmishScenarioRecord>,
    pkt: &IniFile,
    source: SkirmishScenarioSource,
    csf: Option<&CsfFile>,
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
        let (display_name, title_label) = pkt_display_name(pkt, map_stem, csf)
            .unwrap_or_else(|| (display_name_from_basic_or_file(&map_ini, &file_name), None));
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
        record.title_label = title_label;
        let title = std::mem::take(&mut record.display_name);
        finish_pkt_title(&mut record, title);
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
/// through to the next source instead. A label's text comes with the label,
/// which [`relabel_records`] reads again.
fn pkt_display_name(
    pkt: &IniFile,
    map_stem: &str,
    csf: Option<&CsfFile>,
) -> Option<(String, Option<String>)> {
    let section = pkt.section(map_stem)?;
    if let Some(value) = section.read_name("DescriptionText", 0x2C) {
        return Some((decode_widened_utf8(value).into_owned(), None));
    }
    section.read_name("Description", 0x400).map(|label| {
        let text = csf.map_or_else(|| label.to_string(), |csf| csf.text(label).into_owned());
        (text, Some(label.to_string()))
    })
}

fn display_name_from_basic_or_file(ini: &IniFile, file_name: &str) -> String {
    crate::map::basic::parse_basic_section(ini)
        .name
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| file_name.to_string())
}

fn read_ini_file(path: &Path) -> Option<IniFile> {
    crate::util::file_system::read(path)
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

#[cfg(all(test, tests_map))]
mod tests {
    use super::*;
    use crate::map::preview::{PreviewSourceBounds, PreviewStartPoint};
    use crate::map::source::test_support::{TestDirectory, make_new_format_mix_bytes};

    #[test]
    fn arctic_ice_passages_lists_and_loads_from_the_maps_folder() {
        use crate::map::installed::ARCTIC_MAP;
        assert!(crate::map::source::bundled_map_bytes(ARCTIC_MAP).is_none());

        let game = TestDirectory::new("installed-ice-passages");
        let maps = Path::new(env!("CARGO_MANIFEST_DIR")).join("../maps");
        let mut assets = AssetManager::from_loose_root_for_test(game.path());
        let records = skirmish_scenario_records_in(game.path(), &mut assets, None, None).unwrap();
        assert!(!records.iter().any(|row| row.file_name == ARCTIC_MAP));
        game.write(
            "Harbor.map",
            b"[Basic]\nName=Harbor\n[Map]\nSize=0,0,16,16\n",
        );
        let records =
            skirmish_scenario_records_in(game.path(), &mut assets, None, Some(&maps)).unwrap();
        assert_eq!(
            records
                .iter()
                .map(|row| row.file_name.as_str())
                .collect::<Vec<_>>(),
            ["Harbor.map", crate::map::source::ICE_LAKE, ARCTIC_MAP]
        );
        let row = records
            .iter()
            .find(|row| row.file_name == ARCTIC_MAP)
            .unwrap();
        assert_eq!(
            row.source,
            SkirmishScenarioSource::LooseMap(ARCTIC_MAP.into())
        );
        assert_eq!(row.display_name, "Arctic Circle 16 - Ice Passages");
        assert_eq!(row.player_capacity, 30);

        let path = maps.join(&row.file_name);
        let loaded = crate::map::source::load_map_by_name_or_path_with_assets(
            game.path(),
            path.to_str().unwrap(),
            &assets,
        )
        .unwrap();
        assert!(matches!(loaded.source, LoadedMapSource::Loose { .. }));
        assert_eq!(loaded.map.waypoints.len(), 30);
        assert!(crate::map::theater::map_requests_bundled_ice(
            &loaded.map.header.theater,
            &loaded.map.ini,
        ));

        let player_maps = TestDirectory::new("ice-and-custom");
        for name in [ARCTIC_MAP, "Custom.map"] {
            player_maps.write(name, b"[Basic]\nName=Custom\n[Map]\nSize=0,0,16,16\n");
        }
        let records =
            skirmish_scenario_records_in(game.path(), &mut assets, None, Some(player_maps.path()))
                .unwrap();
        assert!(records.iter().any(|row| row.file_name == "Custom.map"));
        assert!(!records.iter().any(|row| row.file_name == "Harbor.map"));
    }

    #[test]
    fn bundled_ice_lake_chooser_launch_and_transfer_share_the_map() {
        use crate::map::source::{
            load_map_by_name_or_path_with_assets, prepare_transfer_map_source,
        };
        let game = TestDirectory::new("bundled-ice-lake");
        game.write(
            "missionsmd.pkt",
            b"[MultiMaps]\n0=Arena\n[Arena]\nGameMode=standard\n",
        );
        game.write("Arena.map", b"[Basic]\nName=Arena\n[Map]\nSize=0,0,16,16\n");
        let mut assets = AssetManager::from_loose_root_for_test(game.path());
        let catalog = |assets: &mut AssetManager| {
            list_skirmish_scenario_records_with_assets(game.path(), assets, None).unwrap()
        };
        let records = catalog(&mut assets);
        assert!(
            records
                .iter()
                .any(|r| r.source == SkirmishScenarioSource::MissionsMdPkt)
        );
        let lake = records
            .iter()
            .find(|r| r.display_name == "Ice Lake")
            .expect("Ice Lake in Skirmish");
        assert_eq!(lake.player_capacity, 2);
        assert_eq!(lake.game_modes, ["standard"]);
        let loaded =
            load_map_by_name_or_path_with_assets(game.path(), &lake.file_name, &assets).unwrap();
        let (len, hash) = loaded.source.physical_identity().unwrap();
        let upload = prepare_transfer_map_source(&loaded.source, &assets).unwrap();
        let bytes = upload.read(hash, len).unwrap();
        assert_eq!(loaded.map.basic.name.as_deref(), Some("Ice Lake"));
        assert_eq!(
            crate::map::map_file::MapFile::from_bytes(&bytes)
                .unwrap()
                .basic
                .name,
            loaded.map.basic.name
        );

        game.write(
            "lakes.yro",
            &make_new_format_mix_bytes(&[(
                &lake.file_name,
                b"[Basic]\nName=Archival lake\n[Map]\nSize=0,0,16,16\n",
            )]),
        );
        let records = catalog(&mut assets);
        let archived = records
            .iter()
            .find(|r| r.file_name == lake.file_name)
            .unwrap();
        assert_eq!(archived.display_name, "Archival lake");
        assert_eq!(archived.source, SkirmishScenarioSource::ArchiveMap);

        game.write(
            &lake.file_name,
            b"[Basic]\nName=Local lake\n[Map]\nSize=0,0,16,16\n",
        );
        let records = catalog(&mut assets);
        let rows: Vec<_> = records
            .iter()
            .filter(|r| r.file_name == lake.file_name)
            .map(|r| (r.display_name.as_str(), &r.source))
            .collect();
        assert_eq!(
            rows,
            [(
                "Local lake",
                &SkirmishScenarioSource::LooseMap(lake.file_name.clone())
            )]
        );
    }

    #[test]
    fn skirmish_lists_custom_map_extensions_in_the_maps_folder() {
        let game = TestDirectory::new("skirmish-game");
        let maps = TestDirectory::new("skirmish-maps");
        let header = b"[Basic]\nName=Harbor\nGameMode=standard\n[Waypoints]\n0=100011\n1=120013\n";
        game.write("Mission.MAP", header);
        maps.write("mission.map", header);
        game.write("Double.map.yrm", header);
        maps.write("double.map", header);
        maps.write("Harbor.MaP", header);
        maps.write(
            "Valley.MpR",
            b"[Map]\nSize=0,0,16,16\n[Basic]\nName=Valley\n",
        );
        maps.write("Winter.yrm", b"[Basic]\nName=Winter\n");
        maps.write("Notes.txt", header);
        maps.write("listed.map", header);

        let mut records = Vec::new();
        append_pkt_records(
            &mut records,
            &IniFile::from_str("[MultiMaps]\n0=Listed\n[Listed]\nDescription=Listed\n"),
            SkirmishScenarioSource::MissionsMdPkt,
            None,
            |_| Some(IniFile::from_str("[Basic]\nName=Listed\n")),
        );
        let assets = AssetManager::from_loose_root_for_test(game.path());
        append_loose_map_records(&mut records, &[game.path()], Some(maps.path()), &assets).unwrap();
        let names: Vec<_> = records.iter().map(|r| r.file_name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Listed.MAP",
                "Double.map.yrm",
                "Harbor.MaP",
                "Valley.MpR",
                "Winter.yrm"
            ]
        );
        assert_eq!(records[2].display_name, "Harbor");
        assert_eq!(records[2].multiplayer_start_waypoints.len(), 2);
    }

    #[test]
    fn asset_aware_catalog_scans_use_the_selected_mod_without_global_setup() {
        use crate::assets::asset_manager::MediaArchiveMode;

        let base = crate::assets::asset_manager::test_support::audio_test_install("catalog-base");
        std::fs::write(
            base.path().join("Arena.MAP"),
            b"[Basic]\nAuthor=Base arena\n",
        )
        .unwrap();
        std::fs::write(
            base.path().join("Shared.yrm"),
            b"[Basic]\nName=Base loose\n",
        )
        .unwrap();
        let selected_mod = TestDirectory::new("catalog-selected-mod");
        selected_mod.write("ARENA.MAP", b"[Basic]\nAuthor=Mod arena\n");
        selected_mod.write("SHARED.YRM", b"[Basic]\nName=Mod loose\n");
        selected_mod.write(
            "MapList.PKT",
            b"[MultiMaps]\n0=Arena\n[Arena]\nDescriptionText=Mod catalog\n",
        );
        selected_mod.write(
            "Packs.YRO",
            &make_new_format_mix_bytes(&[
                (
                    "Packs.PKT",
                    b"[MultiMaps]\n0=Packed\n[Packed]\nDescriptionText=Packed catalog\n",
                ),
                ("Packed.MAP", b"[Basic]\nAuthor=Mod pack\n"),
            ]),
        );
        let mut assets = AssetManager::with_mod(
            base.path(),
            Some(selected_mod.path()),
            MediaArchiveMode::STOCK_DIGITAL,
        )
        .unwrap();

        let records =
            list_skirmish_scenario_records_with_assets(base.path(), &mut assets, None).unwrap();
        assert_eq!(records.len(), 3);
        let record = |name: &str| {
            records
                .iter()
                .find(|record| record.file_name.eq_ignore_ascii_case(name))
                .unwrap()
        };
        assert_eq!(record("Arena.MAP").display_name, "Mod catalog");
        assert_eq!(record("Arena.MAP").author.as_deref(), Some("Mod arena"));
        assert_eq!(record("Packed.MAP").author.as_deref(), Some("Mod pack"));
        assert_eq!(record("Shared.yrm").display_name, "Mod loose");
        assert!(assets.archive("Packs.YRO").is_some());
    }

    #[test]
    fn skirmish_discovers_multiplayer_mpr_files_and_prefers_the_mod_copy() {
        use crate::assets::asset_manager::MediaArchiveMode;

        let game = crate::assets::asset_manager::test_support::audio_test_install("mpr-game");
        let selected_mod = TestDirectory::new("mpr-mod");
        let map = "[Map]\nSize=0,0,16,16\n[Basic]\nName=";
        std::fs::write(
            game.path().join("Harbor.mpr"),
            format!("{map}Game harbor\n"),
        )
        .unwrap();
        std::fs::write(game.path().join("Island.mpr"), format!("{map}Island\n")).unwrap();
        // Looking up the base file's name would load the mod's suffixed file.
        std::fs::write(game.path().join("Cove.mpr"), format!("{map}Base cove\n")).unwrap();
        selected_mod.write("Cove.mpr.yrm", format!("{map}Mod cove\n").as_bytes());
        selected_mod.write("HARBOR.MpR", format!("{map}Mod harbor\n").as_bytes());
        selected_mod.write(
            "Intro.mpr",
            format!("{map}Mission\nMultiplayerOnly=no\n").as_bytes(),
        );
        selected_mod.write("Broken.mpr", b"[Basic]\nName=Missing terrain\n");
        let mut assets = AssetManager::with_mod(
            game.path(),
            Some(selected_mod.path()),
            MediaArchiveMode::STOCK_DIGITAL,
        )
        .unwrap();

        let records =
            list_skirmish_scenario_records_with_assets(game.path(), &mut assets, None).unwrap();
        let names: Vec<_> = records
            .iter()
            .map(|record| record.display_name.as_str())
            .collect();
        assert_eq!(names, ["Mod cove", "Mod harbor", "Island"]);
    }

    #[test]
    fn empty_catalog_fallback_rejects_an_explicit_campaign_mpr() {
        let dir = TestDirectory::new("fallback-campaign-mpr");
        dir.write(
            "Intro.mpr",
            b"[Map]\nSize=0,0,16,16\n[Basic]\nName=Intro\nMultiplayerOnly=no\n",
        );
        assert!(list_available_maps_in(dir.path(), None).unwrap().is_empty());
    }

    #[test]
    fn skirmish_mpr_discovery_uses_the_games_active_include_rules() {
        let dir = TestDirectory::new("mpr-include");
        dir.write("Coast.MPR", b"[#include]\n0=CoastData.ini\n");
        dir.write(
            "CoastData.ini",
            b"[Map]\nSize=0,0,16,16\n[Basic]\nName=Included coast\n",
        );
        let mut assets = AssetManager::from_loose_root_for_test(dir.path());
        for enabled in [false, true] {
            let _ares = crate::rules::ares_switch::read_ares_way(enabled);
            let records =
                list_skirmish_scenario_records_with_assets(dir.path(), &mut assets, None).unwrap();
            assert_eq!(
                records
                    .iter()
                    .find(|r| r.file_name.eq_ignore_ascii_case("Coast.MPR"))
                    .map(|r| r.display_name.as_str()),
                enabled.then_some("Included coast")
            );
        }
    }

    #[test]
    fn declared_catalogs_supply_the_maps_and_modes() {
        let directory = TestDirectory::new("declared-catalogs");
        let pack = make_new_format_mix_bytes(&[
            ("custom-maps.pkt", &b"[MultiMaps]\n0=Arena\n[Arena]\nDescriptionText=Declared arena\nGameMode=custom\nMinPlayers=2\nMaxPlayers=2\n"[..]),
            ("custom-modes.ini", &b"[Battle]\n12=GUI:Custom,STT:Custom,match.ini,custom,false\n"[..]),
            ("match.ini", &b"[MultiplayerDialogSettings]\nAlliesAllowed=no\n"[..]),
            ("Arena.MAP", &b"[Basic]\nName=Arena\n[Waypoints]\n0=100011\n1=120013\n"[..]),
        ]);
        directory.write("resources.yro", &pack);
        let mut assets = AssetManager::from_loose_root_for_test(directory.path())
            .with_ini_roots(&crate::util::ini_roots::IniRootConfig {
                modes: Some("custom-modes.ini".into()),
                map_catalog: Some("custom-maps.pkt".into()),
                ..Default::default()
            })
            .unwrap();
        assets
            .register_loose_yro_archive(&directory.path().join("resources.yro"))
            .unwrap();

        let modes = crate::skirmish_modes::skirmish_modes_from_assets(&assets).unwrap();
        assert_eq!(modes.len(), 1);
        assert_eq!(modes[0].id, 12);
        assert!(!modes[0].allies_allowed);
        let maps = list_skirmish_scenario_records_with_assets(directory.path(), &mut assets, None)
            .unwrap();
        assert_eq!(maps.len(), 1);
        assert_eq!(maps[0].display_name, "Declared arena");
        assert_eq!(maps[0].game_modes, ["custom"]);
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
    fn map_text_saved_as_utf8_reads_as_its_characters() {
        let directory = TestDirectory::new("utf8-map-text");
        let path = directory.path().join("Daxian.yrm");
        std::fs::write(&path, "[Basic]\nName=大仙防守\nAuthor=小明\n").unwrap();
        let entry = read_map_menu_entry(&path, "Daxian.yrm");
        assert_eq!(entry.display_name, "大仙防守");
        assert_eq!(entry.author.as_deref(), Some("小明"));

        // gamemd's 64-byte buffer ends this name inside a character.
        std::fs::write(&path, format!("[Basic]\nName=A{}\n", "大".repeat(30))).unwrap();
        let entry = read_map_menu_entry(&path, "Daxian.yrm");
        assert_eq!(entry.display_name, format!("A{}", "大".repeat(20)));

        let pkt = "[MultiMaps]\n1=Arena\n[Arena]\nDescriptionText=大仙防守\n";
        let pkt = IniFile::from_bytes(pkt.as_bytes()).unwrap();
        let mut records = Vec::new();
        append_pkt_records(
            &mut records,
            &pkt,
            SkirmishScenarioSource::MissionsMdPkt,
            None,
            bare_map_payload,
        );
        assert_eq!(records[0].display_name, "大仙防守");

        assert_eq!(
            crate::map::basic::briefing_text("[Basic]\nBriefing=守住@大桥\n".as_bytes()).as_deref(),
            Some("守住\n大桥")
        );
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
    fn header_start_count_accepts_offline_skirmish_capacity_and_rejects_excess() {
        let capacity = crate::skirmish_launch::SKIRMISH_MAX_SLOT_COUNT;
        let header = |count| {
            IniFile::from_str(&format!(
                "[Header]\nStartX=0\nStartY=0\nWidth=100\nHeight=80\nNumberStartingPoints={count}\n",
            ))
        };
        let entry = read_map_menu_entry_from_ini(&header(capacity), "test.map");
        assert_eq!(
            entry.preview_source_bounds.unwrap().start_points.len(),
            capacity
        );
        let entry = read_map_menu_entry_from_ini(&header(capacity + 1), "test.map");
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
            bare_map_payload,
        );

        // Equal bounds print one number; differing bounds print the span.
        assert_eq!(records[0].display_name, "Dustbowl (2)");
        assert_eq!(records[1].display_name, "Circuit Board (2-4)");
    }

    #[test]
    fn a_language_change_reads_table_titles_again_with_their_yro_span() {
        let pkt = IniFile::from_str(
            "[MultiMaps]\n1=Zoo\n2=Alpha\n\
             [Zoo]\nDescriptionText=Zoo Display\n\
             [Alpha]\nDescription=GUI:AlphaName\nMinPlayers=2\nMaxPlayers=2\n",
        );
        let mut csf = build_test_csf(&[("GUI:AlphaName", "Alpha")]);
        let mut records = Vec::new();
        for source in [
            SkirmishScenarioSource::MissionsMdPkt,
            SkirmishScenarioSource::LooseYro("pack.yro".to_string()),
        ] {
            append_pkt_records(&mut records, &pkt, source, Some(&csf), bare_map_payload);
        }

        csf.set_override("GUI:AlphaName", "Альфа");
        relabel_records(&mut records, &csf);

        let names: Vec<&str> = records
            .iter()
            .map(|record| record.display_name.as_str())
            .collect();
        assert_eq!(
            names,
            ["Zoo Display", "Альфа", "Zoo Display (2-4)", "Альфа (2)"]
        );
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

    #[test]
    fn yro_packs_register_in_native_name_order() {
        // The first pack by name wins an entry both carry. APFS lists these
        // two the other way round.
        let directory = TestDirectory::new("yro-name-order");
        let pack = |body: &[u8]| make_new_format_mix_bytes(&[("SHARED.BIN", body)]);
        directory.write("SECOND.YRO", &pack(b"second"));
        directory.write("First.yro", &pack(b"first"));

        let mut manager = AssetManager::from_loose_root_for_test(directory.path());
        append_loose_yro_records(&mut Vec::new(), directory.path(), &mut manager, None)
            .expect("scan loose YRO");

        assert_eq!(
            manager.registered_archive_names(),
            ["First.yro", "SECOND.YRO"]
        );
        assert_eq!(manager.get_ref("SHARED.BIN"), Some(&b"first"[..]));
    }
}
