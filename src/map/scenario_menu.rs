//! Scenario-menu entry metadata shared by the shell map selector and the
//! skirmish catalog. Map-owned (F06): every field derives from map-file
//! parsing; app initialization only constructs entries.

use crate::map::preview::{PreviewSection, PreviewSourceBounds, PreviewStartPoint};
use crate::map::waypoints::{
    Waypoint, multiplayer_start_waypoints_for_capacity, parse_waypoints, skirmish_player_capacity,
};
use crate::rules::ini_parser::IniFile;

/// A local chooser source's logical identity, independent of chooser rows
/// and of the filename used by the stock multiplayer protocol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalScenarioKey {
    pub source: &'static str,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalScenario {
    pub key: LocalScenarioKey,
    pub mode_label: &'static str,
    pub title: String,
    /// Local labels follow the selection into the frozen start. They do not
    /// participate in the verified inputs or in the setup protocol.
    pub presentation: Option<std::sync::Arc<LocalScenarioPresentation>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalScenarioPresentation {
    pub series_title: String,
    pub stage_title: String,
    pub briefing: Option<String>,
    pub report_strings: std::collections::BTreeMap<String, String>,
    pub role_titles: Vec<Option<String>>,
    /// Opening camera waypoint by canonical primary House, from the admitted manifest.
    pub role_camera_waypoints: std::collections::BTreeMap<String, u32>,
    pub role_appearances: Vec<LocalScenarioRoleAppearance>,
    pub preview: Option<crate::map::preview::DecodedPreview>,
    pub starts: Option<PreviewSourceBounds>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalScenarioRoleAppearance {
    pub country: Option<String>,
    pub color: Option<String>,
    pub opening: LocalScenarioRoleOpening,
    pub shared_forces: Vec<Option<String>>,
    pub resolved_visual: Option<LocalScenarioRoleVisual>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LocalScenarioRoleOpening {
    pub funds: i32,
    pub tech_level: Option<i32>,
    pub units: usize,
    pub buildings: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalScenarioRoleVisual {
    pub flag: Option<String>,
    pub rgb: Option<[u8; 3]>,
}

/// A manifest row in a local source's grouped chooser. Disabled stages retain
/// a row so the displayed sequence does not change with installed files.
#[derive(Clone, Debug)]
pub struct LocalScenarioMenuEntry {
    pub selection: LocalScenario,
    pub group: String,
    pub group_title: String,
    pub stage_title: String,
    pub label: String,
    pub players: String,
    pub unavailable: Option<String>,
    pub no_preview: String,
}

/// Lightweight metadata used by the main-menu map selector.
#[derive(Debug, Clone)]
pub struct MapMenuEntry {
    /// Actual file name/path token used to load the map later.
    pub file_name: String,
    /// Human-facing label derived from `[Basic] Name` when available.
    pub display_name: String,
    /// Optional author text from `[Basic]`.
    pub author: Option<String>,
    /// Lightweight preview metadata from `[Preview]` / `[PreviewPack]`.
    pub preview: PreviewSection,
    /// Multiplayer start waypoints 0..=7, sorted by waypoint index.
    pub multiplayer_start_waypoints: Vec<Waypoint>,
    /// Setup-shell player capacity from native waypoint counting, including
    /// the `[RandomMap] NumPlayers` / eight-player fallback path.
    pub player_capacity: i32,
    /// Verified source bounds for projecting starts onto the preview surface.
    pub preview_source_bounds: Option<PreviewSourceBounds>,
}
pub(crate) fn read_map_menu_entry_from_ini(ini: &IniFile, file_name: &str) -> MapMenuEntry {
    let basic = crate::map::basic::parse_basic_section(ini);
    let display_name = basic
        .name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| file_name.to_string());

    let player_capacity = skirmish_player_capacity(ini);
    MapMenuEntry {
        file_name: file_name.to_string(),
        display_name,
        author: basic.author,
        preview: crate::map::preview::parse_preview_section(ini),
        multiplayer_start_waypoints: multiplayer_start_waypoints_for_capacity(
            &parse_waypoints(ini),
            player_capacity,
        ),
        player_capacity,
        preview_source_bounds: preview_source_bounds_from_verified_source(ini),
    }
}
pub fn preview_source_bounds_from_verified_source(ini: &IniFile) -> Option<PreviewSourceBounds> {
    let header = ini.section("Header")?;
    // `0x00689D8D`-`0x00689E05` ReadInt each field; VERA requires all five.
    let field = |key: &str| header.is_present(key).then(|| header.read_int(key, 0));
    let origin_x = field("StartX")?;
    let origin_y = field("StartY")?;
    let width = field("Width")?;
    let height = field("Height")?;
    let count = field("NumberStartingPoints")?;

    if width <= 0
        || height <= 0
        || count <= 0
        || count > crate::skirmish_launch::SKIRMISH_MAX_SLOT_COUNT as i32
    {
        return None;
    }

    // ReadMinMax over zeroed points (`0x00689D64`, `0x00689E62`).
    let start_points = (1..=count)
        .map(|idx| {
            let [x, y] = header.read_minmax(&format!("Waypoint{idx}"), [0, 0]);
            PreviewStartPoint { x, y }
        })
        .collect();

    Some(PreviewSourceBounds {
        origin_x,
        origin_y,
        width: width as u32,
        height: height as u32,
        start_points,
    })
}
