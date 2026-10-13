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
    let mut starts: Vec<Waypoint> = waypoints
        .values()
        .copied()
        .filter(|wp| MULTIPLAYER_START_WAYPOINTS.contains(&wp.index))
        .collect();
    starts.sort_by_key(|wp| wp.index);
    starts
}

/// Return the first multiplayer/skirmish start waypoint if present.
pub fn first_multiplayer_start(waypoints: &HashMap<u32, Waypoint>) -> Option<Waypoint> {
    multiplayer_start_waypoints(waypoints).into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_waypoints_ra2_format() {
        let ini = IniFile::from_str("[Basic]\nNewINIFormat=5\n[Waypoints]\n0=140034\n99=55098\n");
        let waypoints = parse_waypoints(&ini);
        assert_eq!(waypoints.len(), 2);
        assert_eq!(
            waypoints.get(&0),
            Some(&Waypoint {
                index: 0,
                rx: 34,
                ry: 140
            })
        );
        assert_eq!(
            waypoints.get(&99),
            Some(&Waypoint {
                index: 99,
                rx: 98,
                ry: 55
            })
        );
    }

    #[test]
    fn waypoint_packing_ignores_new_ini_format() {
        // The scenario reader has no NewINIFormat branch: the divisor is 1000
        // whatever the map header claims, so an old-format map decodes the same
        // way a NewINIFormat=5 one does.
        let old = IniFile::from_str("[Basic]\nNewINIFormat=3\n[Waypoints]\n7=2005\n");
        let new = IniFile::from_str("[Basic]\nNewINIFormat=5\n[Waypoints]\n7=2005\n");

        let expected = Waypoint {
            index: 7,
            rx: 5,
            ry: 2,
        };
        assert_eq!(parse_waypoints(&old).get(&7), Some(&expected));
        assert_eq!(parse_waypoints(&new).get(&7), Some(&expected));
    }

    #[test]
    fn waypoint_value_zero_is_unset_not_the_origin_cell() {
        let ini = IniFile::from_str("[Waypoints]\n0=0\n1=100011\n");
        let waypoints = parse_waypoints(&ini);

        assert!(
            !waypoints.contains_key(&0),
            "value 0 is the reader's unset marker"
        );
        assert_eq!(waypoints.len(), 1);
    }

    #[test]
    fn signed_waypoint_values_wrap_quotient_and_remainder_into_raw_u16_halves() {
        let ini = IniFile::from_str(
            "[Waypoints]\n\
             15=$FFFFFFFF\n\
             16=FFFFFC17h\n\
             17=-2000junk\n\
             18=4294967295\n\
             19=junk\n\
             20=$junk\n",
        );
        let waypoints = parse_waypoints(&ini);

        assert_eq!(
            waypoints.get(&15),
            Some(&Waypoint {
                index: 15,
                rx: u16::MAX,
                ry: 0,
            })
        );
        assert_eq!(
            waypoints.get(&16),
            Some(&Waypoint {
                index: 16,
                rx: u16::MAX,
                ry: u16::MAX,
            })
        );
        assert_eq!(
            waypoints.get(&17),
            Some(&Waypoint {
                index: 17,
                rx: 0,
                ry: u16::MAX - 1,
            })
        );
        assert_eq!(
            waypoints.get(&18),
            Some(&Waypoint {
                index: 18,
                rx: u16::MAX,
                ry: 0,
            }),
            "native decimal atoi wraps through i32"
        );
        assert!(!waypoints.contains_key(&19), "junk reads the zero default");
        assert!(
            !waypoints.contains_key(&20),
            "invalid hex reads the zero default"
        );
    }

    #[test]
    fn waypoint_reader_ignores_noncanonical_numeric_key_aliases() {
        let ini = IniFile::from_str(
            "[Waypoints]\n\
             1=300033\n\
             001=100011\n\
             +1=200022\n\
             002=400044\n\
             +3=500055\n",
        );
        let waypoints = parse_waypoints(&ini);

        assert_eq!(
            waypoints,
            [(
                1,
                Waypoint {
                    index: 1,
                    rx: 33,
                    ry: 300,
                },
            )]
            .into_iter()
            .collect(),
            "only the generated exact key `1` may own slot 1; padded/plus aliases remain unread"
        );
    }

    #[test]
    fn waypoint_slots_at_or_above_the_reader_bound_are_ignored() {
        let ini = IniFile::from_str("[Waypoints]\n701=100011\n702=100012\n");
        let waypoints = parse_waypoints(&ini);

        assert!(waypoints.contains_key(&701), "701 is the last slot read");
        assert!(
            !waypoints.contains_key(&702),
            "the reader walks 0..701 and never sees 702"
        );
    }

    #[test]
    fn test_missing_waypoints_is_empty() {
        let ini = IniFile::from_str("[Map]\nTheater=TEMPERATE\n");
        assert!(parse_waypoints(&ini).is_empty());
    }

    #[test]
    fn test_multiplayer_start_waypoints_are_sorted_and_filtered() {
        let ini = IniFile::from_str(
            "[Basic]\nNewINIFormat=5\n[Waypoints]\n11=200300\n3=100200\n0=100050\n99=55098\n",
        );
        let waypoints = parse_waypoints(&ini);
        let starts = multiplayer_start_waypoints(&waypoints);
        assert_eq!(starts.len(), 2);
        assert_eq!(starts[0].index, 0);
        assert_eq!(starts[1].index, 3);
        assert_eq!(first_multiplayer_start(&waypoints), Some(starts[0]));
    }

    #[test]
    fn skirmish_capacity_counts_native_non_minus_one_waypoint_values() {
        let ini = IniFile::from_str("[Waypoints]\n0=100011\n1=-1\n2=-2\n7=120034\n8=130040\n");

        assert_eq!(skirmish_player_capacity(&ini), 3);
    }

    #[test]
    fn skirmish_capacity_uses_random_map_fallback_only_when_no_starts_exist() {
        let fallback = IniFile::from_str("[Waypoints]\n0=-1\n7=-1\n[RandomMap]\nNumPlayers=4\n");
        assert_eq!(skirmish_player_capacity(&fallback), 4);

        let concrete = IniFile::from_str("[Waypoints]\n0=100011\n[RandomMap]\nNumPlayers=6\n");
        assert_eq!(skirmish_player_capacity(&concrete), 1);
    }

    #[test]
    fn skirmish_capacity_zero_or_missing_falls_back_to_eight() {
        assert_eq!(
            skirmish_player_capacity(&IniFile::from_str("[RandomMap]\nNumPlayers=0\n")),
            DEFAULT_SKIRMISH_PLAYER_CAPACITY
        );
        assert_eq!(
            skirmish_player_capacity(&IniFile::from_str("[Basic]\nName=No Starts\n")),
            DEFAULT_SKIRMISH_PLAYER_CAPACITY
        );
    }

    #[test]
    fn skirmish_capacity_preserves_nonzero_signed_random_map_value() {
        assert_eq!(
            skirmish_player_capacity(&IniFile::from_str("[RandomMap]\nNumPlayers=-2\n")),
            -2
        );
    }
}
