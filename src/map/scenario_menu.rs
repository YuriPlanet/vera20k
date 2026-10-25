//! Scenario-menu entry metadata shared by the shell map selector and the
//! skirmish catalog. Map-owned (F06): every field derives from map-file
//! parsing; app initialization only constructs entries.

use crate::map::preview::{PreviewSection, PreviewSourceBounds, PreviewStartPoint};
use crate::map::waypoints::{
    Waypoint, multiplayer_start_waypoints_for_capacity, parse_waypoints, skirmish_player_capacity,
};
use crate::rules::ini_parser::IniFile;

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
        || count > crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT as i32
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
