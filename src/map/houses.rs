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

use crate::rules::color_scheme::ColorSchemeEntry;
use crate::rules::house_colors::{DEFAULT_SCHEME_ENTRY, HouseColorIndex};
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::ini_value::{crt_atoi, strtok};
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
    /// Scenario-authored `IQ=` read into HouseClass CurrentIQ (ReadInt over 0).
    pub iq: i32,
    /// ReadHousesList475260 result, resolved against the complete exact-name
    /// roster. Unknown tokens set bit31; the graph projection admits only
    /// present, non-self House indices.
    pub allies: u32,
    /// Scenario-authored BasePlan in numeric node order.
    pub base_plan: ScenarioBasePlanDefinition,
}

/// Dynamic fields of House ReadScenarioINI500B40. Roster identity, PlayerControl,
/// IQ, Color, alliances and BasePlan are already read by parse_house_roster;
/// the mission-counter default must be supplied by the admitted Scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScenarioHouseParameters {
    pub tech_level: i32,
    pub credits: i32,
    pub edge: i32,
    pub ratio_team_aircraft: i32,
    pub ratio_team_infantry: i32,
    pub ratio_team_units: i32,
}

impl HouseDefinition {
    pub(crate) fn read_scenario_parameters(
        &self,
        ini: &IniFile,
        mission_counter: i32,
    ) -> ScenarioHouseParameters {
        let section = ini.section(&self.name).unwrap_or(IniSection::empty());
        ScenarioHouseParameters {
            tech_level: section.read_int("TechLevel", mission_counter),
            credits: section.read_int("Credits", 0),
            edge: section.read_edge("Edge", -1),
            ratio_team_aircraft: section.read_int("RatioTeamAircraft", 75),
            ratio_team_infantry: section.read_int("RatioTeamInfantry", 75),
            ratio_team_units: section.read_int("RatioTeamUnits", 75),
        }
    }

    /// Resolve the named scenario-house `IQ=` exactly as
    /// `HouseClass::Read_Scenario_INI @ 0x00500B40` does.
    pub const fn scenario_current_iq(&self, max_iq_levels: i32) -> i32 {
        if self.iq > max_iq_levels { 1 } else { self.iq }
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

    /// Original House::FromName50C170 uses byte-exact names in array order.
    pub(crate) fn find_house_index(&self, name: &str) -> Option<usize> {
        self.houses.iter().position(|house| house.name == name)
    }

    /// Project the authored directed graph. Full scenario admission and its
    /// initialization context belong to the simulation diplomacy owner.
    pub fn alliance_map(&self) -> HouseAllianceMap {
        let mut map: HouseAllianceMap = BTreeMap::new();
        for house in &self.houses {
            map.entry(normalize_house_name(&house.name)).or_default();
        }
        for (source_index, house) in self.houses.iter().enumerate() {
            let source = normalize_house_name(&house.name);
            for (target_index, ally) in self.houses.iter().enumerate() {
                // ReadScenarioINI5010DD visits parsed mask bits in House
                // array order. CanAlly501575 rejects self/already-allied.
                if source_index == target_index
                    || house.allies & 1u32.wrapping_shl(target_index as u32) == 0
                {
                    continue;
                }
                let target = normalize_house_name(&ally.name);
                map.entry(source.clone()).or_default().insert(target);
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
#[cfg(test)]
pub fn parse_house_colors(ini: &IniFile, schemes: &[ColorSchemeEntry]) -> HouseColorMap {
    parse_house_roster(ini, schemes, None).color_map()
}

/// Parse the ordered active-house roster from a map's INI data.
///
/// `schemes` is the parsed `[Colors]` list; a house's `Color=<name>` resolves to
/// that entry's index (case-insensitive). The Rules identity owner supplies the
/// Country-derived constructor color; missing/unknown map colors retain it.
/// Callers without Rules use the existing presentation default.
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

    // [Houses] has numbered keys: 0=Americans, 1=Russians, etc. `0x005009B0`
    // walks them by index, each value a 0x14-byte ReadString.
    for key in houses_section.keys() {
        let Some(house_name) = houses_section.read_name(key, 0x14) else {
            continue;
        };
        let house_name = house_name.to_string();

        // `HouseClass::Read_Scenario_INI @ 0x00500B40` reads the house's own
        // section; `Country` is the 0x80-byte index read at `0x00500A05`.
        let section = ini.section(&house_name);
        let fields = section.unwrap_or(IniSection::empty());
        let country = fields.read_name("Country", 0x80).map(str::to_string);
        let current_color = rules.map_or((DEFAULT_SCHEME_ENTRY * 2 + 1) as i32, |rules| {
            rules
                .scenario_country_name(country.as_deref())
                .map_or(0, |name| rules.country_color_scheme(name))
        });
        let native_color = fields.read_color_scheme(
            "Color",
            current_color,
            schemes.iter().map(|scheme| scheme.name.as_str()),
        );
        // House500DF7 uses fallback5 only for a negative index or null
        // scheme. Both shade1/53 members otherwise project to the same
        // logical palette entry; native0 is a valid shade1 scheme.
        let color = usize::try_from(native_color)
            .ok()
            .map(|index| index / 2)
            .filter(|&entry| entry < schemes.len())
            .map_or(HouseColorIndex(DEFAULT_SCHEME_ENTRY as u8), |entry| {
                HouseColorIndex(entry as u8)
            });
        // No native read: Rust keeps `Side` as the side fallback for a country
        // it cannot resolve.
        let side = fields.read_name("Side", 0x80).map(str::to_string);
        let player_control = fields.read_bool_value("PlayerControl");
        let iq = fields.read_int("IQ", 0);
        let base_plan = parse_scenario_base_plan(section, rules);

        houses.push(HouseDefinition {
            name: house_name,
            color,
            country,
            side,
            player_control,
            iq,
            allies: 0,
            base_plan,
        });
    }

    let mut roster = HouseRoster { houses };
    // House Read_INI5009B0 constructs the whole array before any per-House
    // ReadScenarioINI. Resolve 475260 against that same complete identity
    // table, through its sole typed reader. The original caller passes literal
    // zero (XOR EDI501089; PUSH EDI5010A3), independent of both stored masks.
    for index in 0..roster.houses.len() {
        let allies = ini
            .section_or_empty(&roster.houses[index].name)
            .read_houses_list("Allies", 0, |name| roster.find_house_index(name));
        roster.houses[index].allies = allies;
    }
    log::info!(
        "HouseRoster: {} entries parsed from map",
        roster.houses.len()
    );
    roster
}

/// Parse the ordered scenario BasePlan through native
/// `FUN_0042EBE0 @ 0x0042EBE0`. `HouseClass__Read_Scenario_INI @ 0x00500B40`
/// calls it on the embedded base constructed by
/// `HouseClass__Constructor @ 0x004F54A0` through
/// `BaseClass__Constructor @ 0x0042E6F0`.
///
/// Native formats numeric keys as `%03d`, reads each value into `char[128]`
/// (ReadString, 127 payload bytes), classifies a control only when byte zero
/// is `'-'` (`atoi` of the first `strtok` token), otherwise resolves that
/// token through `BuildingTypeClass__FindIndexByName @ 0x0045E7B0`, reads
/// X/Y as `atoi` of the next two tokens narrowed to 16 bits, and appends in
/// numeric-key order. The row read passes a NULL default (`0x0042EC69`);
/// Rust reads an absent row as empty. Its undefined trailing node locals are
/// not scenario fields, so Rust deterministically normalizes filled/retry
/// below.
fn parse_scenario_base_plan(
    section: Option<&IniSection>,
    rules: Option<&RuleSet>,
) -> ScenarioBasePlanDefinition {
    let Some(section) = section else {
        return ScenarioBasePlanDefinition::default();
    };
    let percent_built = section.read_int("PercentBuilt", 0);
    let node_count = section.read_int("NodeCount", 0);
    let mut nodes = Vec::new();
    for index in 0..node_count.max(0) {
        let value = section
            .read_name(&format!("{index:03}"), 0x80)
            .unwrap_or("");
        let mut tokens = strtok(value, &[',']);
        let type_token = tokens.next().unwrap_or("");
        let type_or_control = if value.starts_with('-') {
            crt_atoi(type_token)
        } else {
            rules
                .and_then(|rules| rules.building_type_index(type_token))
                .unwrap_or(-1)
        };
        let x = crt_atoi(tokens.next().unwrap_or(""));
        let y = crt_atoi(tokens.next().unwrap_or(""));
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
        assert_eq!(roster.houses[0].iq, 0);
        assert_eq!(roster.houses[1].allies, 1 << 31);
        let alliances = roster.alliance_map();
        assert!(!are_houses_friendly(
            &alliances,
            "Russians",
            "Confederation"
        ));
        assert!(!are_houses_friendly(&alliances, "YuriCountry", "Russians"));
        assert!(!are_houses_friendly(&alliances, "Americans", "Russians"));
    }

    #[test]
    fn test_alliance_direction_is_asymmetric() {
        let roster = parse_house_roster(
            &IniFile::from_str(
                "[Houses]\n0=Americans\n1=Russians\n[Americans]\nAllies=Russians\n[Russians]\n",
            ),
            &[],
            None,
        );
        let mut alliances = roster.alliance_map();

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
        assert_eq!(roster.houses[0].iq, 5);
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
    fn scenario_house_color_inherits_country_and_retains_unknown_names() {
        // The executed original American controls have no map Color and
        // retain Country Gold: native paired index3, logical palette1.
        // Unknown/empty retention and CI authored lookup follow the same
        // ReadColor474A90 body; no independent campaign parser is involved.
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/input_oracle/campaign_start_houses.json",
        ))
        .unwrap();
        let control = native["controls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "negative_easy")
            .unwrap();
        let expected = HouseColorIndex(
            (control["final_houses"][0]["initial_color_index"]
                .as_u64()
                .unwrap()
                / 2) as u8,
        );
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[Colors]\nLightGold=25,255,255\nGold=43,239,255\n\
             [Countries]\n0=Americans\n[Americans]\nColor=Gold\n",
        ))
        .unwrap();
        for color in [None, Some(""), Some("UnknownName"), Some("gOlD")] {
            let mut section = IniSection::new("ControlHouse".to_string());
            // A missing Country selects the shared entry0 identity; the
            // House's name cannot supply another constructor color.
            if let Some(color) = color {
                section.set("Color", color);
            }
            let mut houses = IniSection::new("Houses".to_string());
            houses.set("0", "ControlHouse");
            let map = IniFile::from_sections_for_test([houses, section]);
            let roster = parse_house_roster(&map, &rules.color_schemes, Some(&rules));
            assert_eq!(roster.houses[0].color, expected, "map Color={color:?}");
        }
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
