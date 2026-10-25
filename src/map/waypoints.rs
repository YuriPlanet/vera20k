//! Map waypoint parsing.
//!
//! Waypoints are numbered cell anchors used by mission logic, spawns, AI teams,
//! and other map-authored behavior. Parsing them now gives later trigger/team
//! work a stable source of truth.

use std::collections::HashMap;

use crate::rules::ini_parser::IniFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Waypoint {
    pub index: u32,
    pub rx: u16,
    pub ry: u16,
}

/// Standard multiplayer/skirmish start waypoints in RA2/YR.
pub const MULTIPLAYER_START_WAYPOINTS: std::ops::RangeInclusive<u32> = 0..=7;

/// Native fallback when a map defines neither a usable `[Waypoints]` start nor
/// a non-zero `[RandomMap] NumPlayers` value.
pub const DEFAULT_SKIRMISH_PLAYER_CAPACITY: i32 = 8;

/// Return the player capacity used by the Skirmish setup shell.
///
/// `gamemd` reads the eight numeric `[Waypoints]` keys with an integer default
/// of `-1` and counts every result other than `-1`; the coordinate itself is not
/// validated by this UI query. Only when that count is zero does it consult
/// `[RandomMap] NumPlayers`, with zero/missing falling back to eight.
pub fn skirmish_player_capacity(ini: &IniFile) -> i32 {
    // Explicit VERA extension. MaxPlayers alone never turns unrelated mission
    // waypoints into starts: every declared starting slot must be authored.
    let declared = ini.section_or_empty("Basic").read_int("MaxPlayers", 0);
    if (9..=crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT as i32).contains(&declared)
        && ini.section("Waypoints").is_some_and(|section| {
            (0..declared).all(|index| !matches!(section.read_int(&index.to_string(), 0), 0 | -1))
        })
    {
        return declared;
    }
    let waypoint_count = ini.section("Waypoints").map_or(0, |section| {
        MULTIPLAYER_START_WAYPOINTS
            .filter(|index| section.read_int(&index.to_string(), -1) != -1)
            .count()
    });
    if waypoint_count != 0 {
        return i32::try_from(waypoint_count).expect("the native query examines only eight keys");
    }

    let random_map_players = ini.section_or_empty("RandomMap").read_int("NumPlayers", 0);
    if random_map_players == 0 {
        DEFAULT_SKIRMISH_PLAYER_CAPACITY
    } else {
        random_map_players
    }
}

/// Number of waypoint slots the scenario reader walks: keys `"0"` through
/// `"701"`. Anything numbered at or above this is never read.
pub(crate) const WAYPOINT_SLOT_COUNT: u32 = 0x2BE;

/// Portable bound for Scenario's fixed waypoint allocation. Native raw
/// accessors outside it address other fields rather than a waypoint.
pub(crate) fn waypoint_slot(index: i32) -> Option<u32> {
    u32::try_from(index)
        .ok()
        .filter(|&index| index < WAYPOINT_SLOT_COUNT)
}

/// IsDefinedWaypoint68BD80: a valid slot differs from empty B05458=(0,0).
/// Both the live Scenario table and authored-map initialization use this
/// decision; their different containers only supply the coordinate lookup.
pub(crate) fn waypoint_is_defined(
    index: i32,
    cell: impl FnOnce(u32) -> Option<(i16, i16)>,
) -> bool {
    waypoint_slot(index)
        .and_then(cell)
        .is_some_and(|cell| cell != (0, 0))
}

/// Packing divisor for a waypoint value: `rx = value % 1000`, `ry = value / 1000`.
///
/// The scenario reader applies this unconditionally — there is no
/// `NewINIFormat` branch and no legacy 128-column packing on this path.
const WAYPOINT_COORD_FACTOR: i32 = 1000;

/// Parse `[Waypoints]` into a waypoint index -> cell mapping.
///
/// Mirrors the scenario waypoint reader: it walks the fixed slot range
/// `0..701`, reads each key as an integer defaulting to zero, treats zero as
/// "unset" rather than as cell `(0, 0)`, and unpacks every other value as
/// signed `rx = value % 1000` / `ry = value / 1000`, then stores both results
/// as their raw wrapped 16-bit halves.
pub fn parse_waypoints(ini: &IniFile) -> HashMap<u32, Waypoint> {
    let Some(section) = ini.section("Waypoints") else {
        return HashMap::new();
    };

    let mut waypoints: HashMap<u32, Waypoint> = HashMap::new();
    for index in 0..WAYPOINT_SLOT_COUNT {
        let key = index.to_string();
        // ScenarioClass__Read_Waypoints @ 0x0068BDC0 calls
        // CCINIClass__ReadInt with each generated canonical decimal key and
        // default zero. Reuse the shared native integer reader for `$FF`/`FFh`,
        // leading atoi, and i32 wrapping.
        let coords = section.read_int(&key, 0);
        // Zero is the reader's "no waypoint here" value, not the origin cell.
        if coords == 0 {
            continue;
        }
        let rx = (coords % WAYPOINT_COORD_FACTOR) as u16;
        let ry = (coords / WAYPOINT_COORD_FACTOR) as u16;
        waypoints.insert(index, Waypoint { index, rx, ry });
    }

    if !waypoints.is_empty() {
        log::info!("Parsed {} waypoints from [Waypoints]", waypoints.len());
    }
    waypoints
}

/// Return multiplayer start waypoints (0..=7) sorted by waypoint index.
pub fn multiplayer_start_waypoints(waypoints: &HashMap<u32, Waypoint>) -> Vec<Waypoint> {
    multiplayer_start_waypoints_for_capacity(waypoints, 8)
}

/// Project only the starts admitted by the map metadata owner. Extended maps
/// explicitly declare 9–30 players; ordinary maps keep their eight-slot range.
pub fn multiplayer_start_waypoints_for_capacity(
    waypoints: &HashMap<u32, Waypoint>,
    capacity: i32,
) -> Vec<Waypoint> {
    let limit = multiplayer_start_slot_limit(capacity);
    let mut starts: Vec<Waypoint> = waypoints
        .values()
        .copied()
        .filter(|wp| wp.index < limit)
        .collect();
    starts.sort_by_key(|wp| wp.index);
    starts
}

/// Authored start slots considered by both Gather and the retained Scenario
/// table. Ordinary maps retain the native eight-slot range.
pub fn multiplayer_start_slot_limit(capacity: i32) -> u32 {
    capacity.clamp(8, crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT as i32) as u32
}

/// Return the first multiplayer/skirmish start waypoint if present.
pub fn first_multiplayer_start(waypoints: &HashMap<u32, Waypoint>) -> Option<Waypoint> {
    multiplayer_start_waypoints(waypoints).into_iter().next()
}

#[cfg(test)]
mod extended_capacity_tests {
    use super::*;

    #[test]
    fn thirty_starts_require_explicit_complete_map_declaration() {
        let starts: String = (0..30)
            .map(|index| format!("{index}={}\n", 100_100 + index))
            .collect();
        let text = format!("[Basic]\nMaxPlayers=30\n[Waypoints]\n{starts}");
        let ini = IniFile::from_str(&text);
        let entry = crate::map::scenario_menu::read_map_menu_entry_from_ini(&ini, "thirty.map");
        assert_eq!(entry.player_capacity, 30);
        assert_eq!(entry.multiplayer_start_waypoints.last().unwrap().index, 29);
        let ordinary = IniFile::from_str(&text.replace("MaxPlayers=30", "MaxPlayers=8"));
        assert_eq!(skirmish_player_capacity(&ordinary), 8);
        let incomplete = IniFile::from_str(&text.replace("29=100129\n", ""));
        assert_eq!(skirmish_player_capacity(&incomplete), 8);
    }
}
