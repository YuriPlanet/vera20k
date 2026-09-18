//! Map house parsing â€” extracts active house definitions and color assignments.
//!
//! RA2 map files have a `[Houses]` section that lists active factions/owners.
//! Each house also has its own section with keys such as `Color=`, `Country=`,
//! `Side=`, `Allies=`, and sometimes `PlayerControl=`.
//!
//! This module parses those sections into a `HouseRoster` plus the derived
//! `HouseColorMap`. The roster keeps the original map order and the most useful
//! ownership metadata for later simulation/UI work.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::rules::color_scheme::{ColorSchemeEntry, scheme_entry_by_name};
use crate::rules::house_colors::{DEFAULT_SCHEME_ENTRY, HouseColorIndex};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;

/// Ordered map-side BasePlan node, installed into House simulation state before
/// map objects are spawned.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScenarioBasePlanNode {
    pub type_or_control: i32,
    pub packed_cell: u32,
    pub filled: bool,
    pub retry_count: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScenarioBasePlanDefinition {
    pub percent_built: i32,
    pub nodes: Vec<ScenarioBasePlanNode>,
}

const fn pack_scenario_base_plan_cell(x: i32, y: i32) -> u32 {
    (x as i16 as u16 as u32) | ((y as i16 as u16 as u32) << 16)
}

/// Mapping from owner name (e.g., "Americans") to house color index.
///
/// Used at atlas build time to determine which palette ramp to apply,
/// and at render time for minimap dot colors.
pub type HouseColorMap = HashMap<String, HouseColorIndex>;
/// Normalized alliance graph keyed by uppercase house name.
pub type HouseAllianceMap = BTreeMap<String, BTreeSet<String>>;

/// Parsed metadata for one active house listed in `[Houses]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HouseDefinition {
    /// House section name as referenced by entities and triggers.
    pub name: String,
    /// House color selection used for remap rendering.
    pub color: HouseColorIndex,
    /// Optional country/country-like identity from `Country=`.
    pub country: Option<String>,
    /// Optional side/faction grouping from `Side=`.
    pub side: Option<String>,
    /// Optional player-control hint from `PlayerControl=`.
    pub player_control: Option<bool>,
    /// Optional scenario-authored `IQ=` read into HouseClass CurrentIQ.
    pub iq: Option<i32>,
    /// Allies listed in the house section.
    pub allies: Vec<String>,
    /// Scenario-authored BasePlan in numeric node order.
    pub base_plan: ScenarioBasePlanDefinition,
}

impl HouseDefinition {
    /// Resolve the named scenario-house `IQ=` exactly as
    /// `HouseClass::Read_Scenario_INI @ 0x00500B40` does.
    pub const fn scenario_current_iq(&self, max_iq_levels: i32) -> i32 {
        match self.iq {
            Some(iq) if iq > max_iq_levels => 1,
            Some(iq) => iq,
            None => 0,
        }
    }
}

/// Ordered active-house list from the map's `[Houses]` section.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HouseRoster {
    /// Houses in the same order they appear in `[Houses]`.
    pub houses: Vec<HouseDefinition>,
}

impl HouseRoster {
    /// Convert roster entries to the color map used by render code.
    pub fn color_map(&self) -> HouseColorMap {
        self.houses
            .iter()
            .map(|house| (house.name.clone(), house.color))
            .collect()
    }

    /// Convert roster entries to a symmetric alliance graph.
    pub fn alliance_map(&self) -> HouseAllianceMap {
        let mut map: HouseAllianceMap = BTreeMap::new();
        for house in &self.houses {
            map.entry(normalize_house_name(&house.name)).or_default();
        }
        for house in &self.houses {
            let source = normalize_house_name(&house.name);
            for ally in &house.allies {
                let target = normalize_house_name(ally);
                map.entry(source.clone())
                    .or_default()
                    .insert(target.clone());
                map.entry(target).or_default().insert(source.clone());
            }
        }
        map
    }
}

/// Returns true when two house names should be treated as friendly.
pub fn are_houses_friendly(alliances: &HouseAllianceMap, a: &str, b: &str) -> bool {
    if a.eq_ignore_ascii_case(b) {
        return true;
    }
    let a_norm = normalize_house_name(a);
    let b_norm = normalize_house_name(b);
    alliances
        .get(&a_norm)
        .is_some_and(|set| set.contains(&b_norm))
        || alliances
            .get(&b_norm)
            .is_some_and(|set| set.contains(&a_norm))
}

/// Directional alliance test: does `asker` consider `other` an ally?
///
/// gamemd's `HouseClass::IsAlliedWith` reads only the *asker's* own ally
/// bitfield, so alliance is one-way until both sides set their bit; a house is
/// always allied with itself. [`are_houses_friendly`] deliberately keeps its
/// symmetric OR for the many "don't shoot / don't crush / don't block" call
/// sites; use this one where the native code needs the asymmetric answer.
pub fn is_allied_with(alliances: &HouseAllianceMap, asker: &str, other: &str) -> bool {
    if asker.eq_ignore_ascii_case(other) {
        return true;
    }
    alliances
        .get(&normalize_house_name(asker))
        .is_some_and(|set| set.contains(&normalize_house_name(other)))
}

/// Mutual-alliance test — both houses must name each other.
///
/// This is the pairwise predicate the native game-over scan applies to every
/// surviving house pair, and it is strictly stronger than
/// [`are_houses_friendly`]: a one-way alliance does not end the match.
pub fn are_houses_mutually_allied(alliances: &HouseAllianceMap, a: &str, b: &str) -> bool {
    is_allied_with(alliances, a, b) && is_allied_with(alliances, b, a)
}

fn normalize_house_name(name: &str) -> String {
    name.trim().to_ascii_uppercase()
}

/// Parse house color assignments from a map's INI data.
///
/// This remains as a compatibility helper for systems that only need color.
/// `schemes` is the parsed `[Colors]` list used to resolve each house's
/// `Color=<name>` to a `[Colors]` entry index.
pub fn parse_house_colors(ini: &IniFile, schemes: &[ColorSchemeEntry]) -> HouseColorMap {
    parse_house_roster(ini, schemes, None).color_map()
}

/// Parse the ordered active-house roster from a map's INI data.
///
/// `schemes` is the parsed `[Colors]` list; a house's `Color=<name>` resolves to
/// that entry's index (case-insensitive). Houses with no/unknown color fall back
/// to [`DEFAULT_SCHEME_ENTRY`].
pub fn parse_house_roster(
    ini: &IniFile,
    schemes: &[ColorSchemeEntry],
    rules: Option<&RuleSet>,
) -> HouseRoster {
    let houses_section = match ini.section("Houses") {
        Some(s) => s,
        None => {
            log::info!("No [Houses] section in map â€” all entities use default Gold color");
            return HouseRoster::default();
        }
    };

    let mut houses = Vec::new();

    // [Houses] has numbered keys: 0=Americans, 1=Russians, etc.
    for key in houses_section.keys() {
        let Some(house_name) = houses_section.get(key) else {
            continue;
        };
        let house_name = house_name.trim().to_string();
        if house_name.is_empty() {
            continue;
        }

        let section = ini.section(&house_name);
        let color = section
            .and_then(|s| s.get("Color"))
            .and_then(|name| scheme_entry_by_name(schemes, name))
            .map(|entry| HouseColorIndex(entry as u8))
            .unwrap_or(HouseColorIndex(DEFAULT_SCHEME_ENTRY as u8));
        let country = section.and_then(|s| s.get("Country")).map(str::to_string);
        let side = section.and_then(|s| s.get("Side")).map(str::to_string);
        let player_control = section.and_then(|s| s.get_bool("PlayerControl"));
        let iq = section.and_then(|s| s.get_i32("IQ"));
        let allies = section
            .and_then(|s| s.get_list("Allies"))
            .unwrap_or_default()
            .into_iter()
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        let base_plan = parse_scenario_base_plan(section, rules);

        houses.push(HouseDefinition {
            name: house_name,
            color,
            country,
            side,
            player_control,
            iq,
            allies,
            base_plan,
        });
    }

    log::info!("HouseRoster: {} entries parsed from map", houses.len());
    HouseRoster { houses }
}

/// Parse the ordered scenario BasePlan through native
/// `FUN_0042EBE0 @ 0x0042EBE0`. `HouseClass__Read_Scenario_INI @ 0x00500B40`
/// calls it on the embedded base constructed by
/// `HouseClass__Constructor @ 0x004F54A0` through
/// `BaseClass__Constructor @ 0x0042E6F0`.
///
/// Native formats numeric keys as `%03d`, reads each value into `char[128]`
/// (127 payload bytes before NUL), classifies a control only when byte zero is
/// `'-'`, otherwise resolves `BuildingTypeClass__FindIndexByName @ 0x0045E7B0`,
/// comma-tokenizes, applies signed `atoi`, narrows X/Y to 16 bits, and appends
/// in numeric-key order. Its undefined trailing node locals are not scenario
/// fields, so Rust deterministically normalizes filled/retry below.
fn parse_scenario_base_plan(
    section: Option<&crate::rules::ini_parser::IniSection>,
    rules: Option<&RuleSet>,
) -> ScenarioBasePlanDefinition {
    const NATIVE_ROW_PAYLOAD_BYTES: usize = 127;

    let Some(section) = section else {
        return ScenarioBasePlanDefinition::default();
    };
    let percent_built = section.get_i32("PercentBuilt").unwrap_or(0);
    let node_count = section.get_i32("NodeCount").unwrap_or(0);
    let mut nodes = Vec::new();
    for index in 0..node_count.max(0) {
        let key = format!("{index:03}");
        let value = section.get(&key).unwrap_or("");
        let value = value.as_bytes();
        let value = &value[..value.len().min(NATIVE_ROW_PAYLOAD_BYTES)];
        let mut tokens = value.split(|byte| *byte == b',');
        let type_token = std::str::from_utf8(tokens.next().unwrap_or_default()).unwrap_or("");
        let type_or_control = if value.first() == Some(&b'-') {
            crate::rules::ini_value::atoi_lenient(type_token)
        } else {
            rules
                .and_then(|rules| rules.building_type_index(type_token))
                .unwrap_or(-1)
        };
        let x = atoi_scenario_base_plan_coordinate(tokens.next().unwrap_or_default());
        let y = atoi_scenario_base_plan_coordinate(tokens.next().unwrap_or_default());
        nodes.push(ScenarioBasePlanNode {
            type_or_control,
            packed_cell: pack_scenario_base_plan_cell(x, y),
            // FUN_0042EBE0 assembly 0x0042ED23..0x0042ED2E copies undefined
            // stack locals here; its writer and checksum omit both fields.
            filled: false,
            retry_count: 0,
        });
    }
    ScenarioBasePlanDefinition {
        percent_built,
        nodes,
    }
}

/// Coordinate-token projection through the CRT `atoi @ 0x007C9B72` reached by
/// `FUN_0042EBE0 @ 0x0042EBE0`. CRT `atoi` skips only its ASCII whitespace set
/// before inspecting the optional sign and leading decimal digits.
fn atoi_scenario_base_plan_coordinate(value: &[u8]) -> i32 {
    let leading_whitespace = value
        .iter()
        .take_while(|&&byte| matches!(byte, b'\t'..=b'\r' | b' '))
        .count();
    let value = &value[leading_whitespace..];
    let sign_bytes = usize::from(
        value
            .first()
            .is_some_and(|byte| matches!(*byte, b'-' | b'+')),
    );
    let digit_bytes = value[sign_bytes..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    let numeric_bytes = &value[..sign_bytes + digit_bytes];
    let numeric = std::str::from_utf8(numeric_bytes).expect("BasePlan numeric grammar is ASCII");
    crate::rules::ini_value::atoi_lenient(numeric)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_plan_rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[BuildingTypes]\n0=GAPOWR\n1=GACNST\n\
             [GAPOWR]\nStrength=750\n\
             [GACNST]\nStrength=1000\n",
        ))
        .expect("base-plan rules")
    }

    /// Retail `[Colors]` list (declaration order) — only the entries the tests
    /// reference need exact positions: DarkRed = entry 5, DarkBlue = entry 10.
    fn test_schemes() -> Vec<ColorSchemeEntry> {
        let raw: &[(&str, [u8; 3])] = &[
            ("LightGold", [25, 255, 255]),  // 0
            ("Gold", [43, 239, 255]),       // 1
            ("LightGrey", [0, 0, 240]),     // 2
            ("Grey", [0, 0, 131]),          // 3
            ("Red", [20, 255, 184]),        // 4
            ("DarkRed", [0, 230, 255]),     // 5
            ("Orange", [25, 230, 255]),     // 6
            ("Magenta", [221, 102, 255]),   // 7
            ("Purple", [201, 201, 189]),    // 8
            ("LightBlue", [119, 143, 255]), // 9
            ("DarkBlue", [153, 214, 212]),  // 10
            ("NeonBlue", [185, 156, 238]),  // 11
            ("DarkSky", [131, 200, 230]),   // 12
            ("Green", [104, 241, 195]),     // 13
            ("DarkGreen", [81, 200, 210]),  // 14
        ];
        raw.iter()
            .map(|(name, hsv)| ColorSchemeEntry {
                name: name.to_string(),
                hsv: *hsv,
            })
            .collect()
    }

    #[test]
    fn test_parse_standard_houses() {
        let ini: IniFile = IniFile::from_str(
            "[Houses]\n0=Americans\n1=Russians\n\
             [Americans]\nColor=DarkBlue\nSide=Allies\nCountry=America\nPlayerControl=yes\n\
             [Russians]\nColor=DarkRed\nSide=Soviet\nCountry=Russia\nAllies=Confederation,YuriCountry\n",
        );
        let roster = parse_house_roster(&ini, &test_schemes(), None);
        let map = roster.color_map();
        assert_eq!(map.len(), 2);
        // Color=name resolves to the [Colors] entry index.
        assert_eq!(map["Americans"], HouseColorIndex(10)); // DarkBlue
        assert_eq!(map["Russians"], HouseColorIndex(5)); // DarkRed
        assert_eq!(roster.houses[0].side.as_deref(), Some("Allies"));
        assert_eq!(roster.houses[0].country.as_deref(), Some("America"));
        assert_eq!(roster.houses[0].player_control, Some(true));
        assert_eq!(roster.houses[0].iq, None);
        assert_eq!(
            roster.houses[1].allies,
            vec!["Confederation".to_string(), "YuriCountry".to_string()]
        );
        let alliances = roster.alliance_map();
        assert!(are_houses_friendly(&alliances, "Russians", "Confederation"));
        assert!(are_houses_friendly(&alliances, "YuriCountry", "Russians"));
        assert!(!are_houses_friendly(&alliances, "Americans", "Russians"));
    }

    #[test]
    fn test_alliance_direction_is_asymmetric() {
        // Built by hand rather than via `alliance_map()`, which symmetrizes.
        let mut alliances = HouseAllianceMap::new();
        alliances
            .entry("AMERICANS".to_string())
            .or_default()
            .insert("RUSSIANS".to_string());
        alliances.entry("RUSSIANS".to_string()).or_default();

        assert!(is_allied_with(&alliances, "Americans", "Russians"));
        assert!(!is_allied_with(&alliances, "Russians", "Americans"));
        assert!(is_allied_with(&alliances, "Russians", "Russians"));
        assert!(!are_houses_mutually_allied(
            &alliances,
            "Americans",
            "Russians"
        ));
        // The symmetric helper still answers "friendly" for the same pair.
        assert!(are_houses_friendly(&alliances, "Russians", "Americans"));

        alliances
            .entry("RUSSIANS".to_string())
            .or_default()
            .insert("AMERICANS".to_string());
        assert!(are_houses_mutually_allied(
            &alliances,
            "Americans",
            "Russians"
        ));
    }

    #[test]
    fn test_missing_color_defaults_to_default_scheme() {
        let ini: IniFile = IniFile::from_str("[Houses]\n0=Neutral\n[Neutral]\nIQ=5\n");
        let roster = parse_house_roster(&ini, &test_schemes(), None);
        let map = roster.color_map();
        assert_eq!(map["Neutral"], HouseColorIndex(DEFAULT_SCHEME_ENTRY as u8));
        assert_eq!(roster.houses[0].iq, Some(5));
        assert_eq!(roster.houses[0].scenario_current_iq(5), 5);
        assert_eq!(roster.houses[0].scenario_current_iq(4), 1);
    }

    #[test]
    fn test_unknown_color_defaults_to_default_scheme() {
        let ini: IniFile =
            IniFile::from_str("[Houses]\n0=Neutral\n[Neutral]\nColor=PinkPolkaDot\n");
        let map = parse_house_colors(&ini, &test_schemes());
        assert_eq!(map["Neutral"], HouseColorIndex(DEFAULT_SCHEME_ENTRY as u8));
    }

    #[test]
    fn test_missing_houses_section() {
        let ini: IniFile = IniFile::from_str("[General]\nKey=Value\n");
        let roster = parse_house_roster(&ini, &test_schemes(), None);
        assert!(roster.houses.is_empty());
    }

    #[test]
    fn test_house_without_section() {
        let ini: IniFile = IniFile::from_str("[Houses]\n0=Ghost\n");
        let map = parse_house_colors(&ini, &test_schemes());
        assert_eq!(map["Ghost"], HouseColorIndex(DEFAULT_SCHEME_ENTRY as u8));
    }

    #[test]
    fn gsi_04_05_scenario_nodecount_parses_percent_and_numbered_nodes_in_source_order() {
        let rules = base_plan_rules();
        let ini = IniFile::from_str(
            "[Houses]\n0=AIHouse\n\
             [AIHouse]\nPercentBuilt=-17\nNodeCount=3\n\
             002=GACNST,32768,-32770\n\
             000=GAPOWR,1,2\n\
             001=-5,-32769,65535\n",
        );
        let roster = parse_house_roster(&ini, &test_schemes(), Some(&rules));
        let plan = &roster.houses[0].base_plan;
        assert_eq!(plan.percent_built, -17);
        assert_eq!(
            plan.nodes
                .iter()
                .map(|node| node.type_or_control)
                .collect::<Vec<_>>(),
            [0, -5, 1]
        );
        assert_eq!(plan.nodes[0].packed_cell, 1u32 | (2u32 << 16));
        assert_eq!(plan.nodes[1].packed_cell, 32_767u32 | (65_535u32 << 16));
        assert_eq!(plan.nodes[2].packed_cell, 32_768u32 | (32_766u32 << 16));
        assert!(plan.nodes.iter().all(|node| !node.filled));
        assert!(plan.nodes.iter().all(|node| node.retry_count == 0));
    }

    #[test]
    fn gsi_04_05_scenario_base_plan_coordinates_use_crt_atoi_whitespace() {
        let rules = base_plan_rules();
        let ini = IniFile::from_str(
            "[Houses]\n0=AIHouse\n\
             [AIHouse]\nNodeCount=3\n\
             000=GAPOWR, 10, -11\n\
             001=GACNST,\t+12,\t-13\n\
             002=GAPOWR, 32768, -32770\n",
        );
        let roster = parse_house_roster(&ini, &test_schemes(), Some(&rules));
        let plan = &roster.houses[0].base_plan;

        assert_eq!(plan.nodes[0].packed_cell, 10u32 | (65_525u32 << 16));
        assert_eq!(plan.nodes[1].packed_cell, 12u32 | (65_523u32 << 16));
        assert_eq!(plan.nodes[2].packed_cell, 32_768u32 | (32_766u32 << 16));
        assert_eq!(
            plan.nodes
                .iter()
                .map(|node| node.type_or_control)
                .collect::<Vec<_>>(),
            [0, 1, 0]
        );

        assert_eq!(
            atoi_scenario_base_plan_coordinate(b" \t\n\x0b\x0c\r-14tail"),
            -14
        );
    }

    #[test]
    fn gsi_04_05_scenario_base_plan_type_lookup_preserves_native_token_whitespace() {
        let rules = base_plan_rules();
        let ini = IniFile::from_str(
            "[Houses]\n0=AIHouse\n\
             [AIHouse]\nNodeCount=3\n\
             000=GAPOWR,1,2\n\
             001=GAPOWR ,3,4\n\
             002=GACNST\t,5,6\n",
        );
        let roster = parse_house_roster(&ini, &test_schemes(), Some(&rules));
        let plan = &roster.houses[0].base_plan;

        assert_eq!(
            plan.nodes
                .iter()
                .map(|node| node.type_or_control)
                .collect::<Vec<_>>(),
            [0, -1, -1],
            "native passes the comma-delimited type token to FindIndexByName without trimming"
        );
    }

    #[test]
    fn gsi_04_05_scenario_base_plan_row_uses_native_127_byte_prefix() {
        let rules = base_plan_rules();
        let oversized = format!("GAPOWR,{}98,77", " ".repeat(119));
        assert_eq!(oversized.as_bytes()[126], b'9');
        assert_eq!(oversized.as_bytes()[127], b'8');
        let ini = IniFile::from_str(&format!(
            "[Houses]\n0=AIHouse\n\
             [AIHouse]\nNodeCount=3\n\
             000={oversized}\n\
             001=-5, 32768, -32770\n\
             002=GACNST,\t+12,\t-13\n"
        ));
        let roster = parse_house_roster(&ini, &test_schemes(), Some(&rules));
        let plan = &roster.houses[0].base_plan;

        assert_eq!(
            plan.nodes
                .iter()
                .map(|node| node.type_or_control)
                .collect::<Vec<_>>(),
            [0, -5, 1]
        );
        assert_eq!(plan.nodes[0].packed_cell, 9);
        assert_eq!(plan.nodes[1].packed_cell, 32_768u32 | (32_766u32 << 16));
        assert_eq!(plan.nodes[2].packed_cell, 12u32 | (65_523u32 << 16));
    }
}
