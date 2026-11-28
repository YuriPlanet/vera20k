//! Map waypoint parsing.
//!
//! Waypoints are numbered cell anchors used by mission logic, spawns, AI teams,
//! and other map-authored behavior. Parsing them now gives later trigger/team
//! work a stable source of truth.

// Outside data reaches this module: it refuses bad input, never panics.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::panic,
        clippy::unreachable,
        clippy::indexing_slicing
    )
)]

use std::collections::HashMap;

use crate::rules::ini_parser::{IniFile, IniSection};

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
    if (9..=crate::skirmish_launch::SKIRMISH_MAX_SLOT_COUNT as i32).contains(&declared)
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

const WAYPOINT_RADIX: u32 = 26;

/// The stock scenario reader handles the slots represented by A through ZZ.
const STOCK_WAYPOINT_SLOT_COUNT: u32 = WAYPOINT_RADIX + WAYPOINT_RADIX.pow(2);

/// VERA extends map-authored waypoints through ZZZ.
pub(crate) const WAYPOINT_SLOT_COUNT: u32 = STOCK_WAYPOINT_SLOT_COUNT + WAYPOINT_RADIX.pow(3);

/// Decode an alphabetic waypoint shared by trigger actions and TeamTypes.
/// `FUN_00763690` reads up to two ASCII letters from the start of the token.
/// VERA recognizes exact three-letter labels even with surrounding whitespace;
/// all other tokens go through the original prefix reader unchanged.
pub fn decode_waypoint_token(token: &str) -> Option<u32> {
    if let &[a, b, c] = token.trim_ascii().as_bytes()
        && let (Some(a), Some(b), Some(c)) =
            (waypoint_letter(a), waypoint_letter(b), waypoint_letter(c))
    {
        return Some(STOCK_WAYPOINT_SLOT_COUNT + (a * WAYPOINT_RADIX + b) * WAYPOINT_RADIX + c);
    }

    let mut bytes = token.bytes();
    let a = waypoint_letter(bytes.next()?)?;
    Some(
        bytes
            .next()
            .and_then(waypoint_letter)
            .map_or(a, |b| (a + 1) * WAYPOINT_RADIX + b),
    )
}

fn waypoint_letter(byte: u8) -> Option<u32> {
    let digit = u32::from(byte.to_ascii_uppercase().checked_sub(b'A')?);
    (digit < WAYPOINT_RADIX).then_some(digit)
}

/// Portable bound for Scenario's waypoint allocation.
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

fn read_waypoint(section: &IniSection, index: u32, key: &str) -> Option<Waypoint> {
    let coords = section.read_int(key, 0);
    // Zero is unset; other values are unpacked as signed coordinates and wrap
    // to the low 16 bits, including negative inputs to the native reader.
    (coords != 0).then(|| Waypoint {
        index,
        rx: (coords % WAYPOINT_COORD_FACTOR) as u16,
        ry: (coords / WAYPOINT_COORD_FACTOR) as u16,
    })
}

fn read_numbered_waypoints(section: &IniSection, end: u32) -> impl Iterator<Item = Waypoint> + '_ {
    (0..end).filter_map(move |index| read_waypoint(section, index, &index.to_string()))
}

/// Parse `[Waypoints]` into a waypoint index -> cell mapping.
///
/// Mirrors ScenarioClass__Read_Waypoints @ 0x0068BDC0. Stock slots are
/// generated as canonical decimal keys. VERA's extension also reads authored
/// numeric keys through the ZZZ slot.
pub fn parse_waypoints(ini: &IniFile) -> HashMap<u32, Waypoint> {
    let Some(section) = ini.section("Waypoints") else {
        return HashMap::new();
    };

    // The extension stays sparse. It examines authored decimal keys above the
    // stock range instead of probing every one of the 17,576 extra slots.
    let extended = section.keys().filter_map(|key| {
        // As with generated stock keys, leading zeroes and signs are not keys.
        if !matches!(key.as_bytes().first(), Some(b'1'..=b'9')) {
            return None;
        }
        let index = key.parse::<u32>().ok()?;
        if (STOCK_WAYPOINT_SLOT_COUNT..WAYPOINT_SLOT_COUNT).contains(&index) {
            read_waypoint(section, index, key)
        } else {
            None
        }
    });
    let waypoints: HashMap<_, _> = read_numbered_waypoints(section, STOCK_WAYPOINT_SLOT_COUNT)
        .chain(extended)
        .map(|waypoint| (waypoint.index, waypoint))
        .collect();

    if !waypoints.is_empty() {
        log::info!("Parsed {} waypoints from [Waypoints]", waypoints.len());
    }
    waypoints
}

/// Read only the admitted start slots for the map selector, in slot order.
pub(crate) fn read_multiplayer_start_waypoints(ini: &IniFile, capacity: i32) -> Vec<Waypoint> {
    let Some(section) = ini.section("Waypoints") else {
        return Vec::new();
    };
    read_numbered_waypoints(section, multiplayer_start_slot_limit(capacity)).collect()
}

/// Return multiplayer start waypoints (0..=7) sorted by waypoint index.
pub fn multiplayer_start_waypoints(waypoints: &HashMap<u32, Waypoint>) -> Vec<Waypoint> {
    multiplayer_start_waypoints_for_capacity(waypoints, 8)
}

/// Project only the starts admitted by the map metadata owner. Extended maps
/// explicitly declare 9 to
/// [`crate::skirmish_launch::SKIRMISH_MAX_SLOT_COUNT`] players; ordinary maps
/// keep their eight-slot range.
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
/// table. Ordinary maps retain the native eight-slot range; VERA's offline
/// Skirmish admits up to [`crate::skirmish_launch::SKIRMISH_MAX_SLOT_COUNT`].
pub fn multiplayer_start_slot_limit(capacity: i32) -> u32 {
    capacity.clamp(8, crate::skirmish_launch::SKIRMISH_MAX_SLOT_COUNT as i32) as u32
}

/// Return the first multiplayer/skirmish start waypoint if present.
pub fn first_multiplayer_start(waypoints: &HashMap<u32, Waypoint>) -> Option<Waypoint> {
    multiplayer_start_waypoints(waypoints).into_iter().next()
}

#[cfg(all(test, tests_map))]
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

    #[test]
    fn one_hundred_twenty_starts_fill_the_offline_skirmish_rows() {
        let starts: String = (0..120)
            .map(|index| format!("{index}={}\n", 100_100 + index))
            .collect();
        let text = format!("[Basic]\nMaxPlayers=120\n[Waypoints]\n{starts}");
        let ini = IniFile::from_str(&text);
        let entry = crate::map::scenario_menu::read_map_menu_entry_from_ini(&ini, "wide.map");
        assert_eq!(entry.player_capacity, 120);
        assert_eq!(entry.multiplayer_start_waypoints.last().unwrap().index, 119);
        // One past the offline row count is no extension.
        let past = IniFile::from_str(&text.replace("MaxPlayers=120", "MaxPlayers=121"));
        assert_eq!(skirmish_player_capacity(&past), 8);
    }
}
