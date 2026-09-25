//! Source-ordered scenario records for the Skirmish Choose Map modal.
//!
//! Retail Choose Map consumes a scenario-record list in source append order.
//! This module models that list separately from the legacy display-sorted
//! `available_maps` menu list.

use crate::map::scenario_menu::MapMenuEntry;
use crate::map::briefing::BriefingSection;
use crate::map::preview::{PreviewSection, PreviewSourceBounds};
use crate::map::waypoints::Waypoint;
use crate::rules::ini_parser::IniFile;
use crate::skirmish_modes::SkirmishGameMode;

pub const RANDMAP_SED: &str = "RandMap.Sed";
/// `[RandomMap] NumPlayers` is clamped to this inclusive range before
/// generation, so the sentinel has to advertise the same span.
pub const RANDOM_MAP_MIN_PLAYERS: u8 = 2;
pub const RANDOM_MAP_MAX_PLAYERS: u8 = 8;
/// Start slots the generator produces before a `.SED` supplies `NumPlayers`.
/// This is a *default quota*, not a capacity limit: `RANDOM_MAP_MAX_PLAYERS`
/// used to alias it, which capped the sentinel at 4 and made 5-8 player random
/// maps unselectable.
pub const RANDOM_MAP_GENERATED_START_QUOTA: u8 = 4;

/// Player-count bounds the PKT-entry record constructor seeds before reading
/// the PKT section, so an entry that omits either key keeps these.
pub const PKT_DEFAULT_MIN_PLAYERS: u8 = 2;
pub const PKT_DEFAULT_MAX_PLAYERS: u8 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkirmishScenarioSource {
    MissionsMdPkt,
    LoosePkt(String),
    LooseYro(String),
    LooseYrm(String),
    Synthetic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkirmishScenarioKind {
    ConcreteMap,
    RandomMapSentinel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkirmishScenarioRecord {
    pub source_ordinal: usize,
    pub source: SkirmishScenarioSource,
    pub file_name: String,
    pub display_name: String,
    pub author: Option<String>,
    pub briefing: BriefingSection,
    pub preview: PreviewSection,
    pub multiplayer_start_waypoints: Vec<Waypoint>,
    pub player_capacity: i32,
    pub preview_source_bounds: Option<PreviewSourceBounds>,
    pub game_modes: Vec<String>,
    pub min_players: Option<u8>,
    pub max_players: Option<u8>,
    pub official: bool,
    pub kind: SkirmishScenarioKind,
}

impl SkirmishScenarioRecord {
    pub fn concrete_from_ini(
        source_ordinal: usize,
        source: SkirmishScenarioSource,
        file_name: &str,
        ini: &IniFile,
    ) -> Self {
        let entry = crate::map::scenario_menu::read_map_menu_entry_from_ini(ini, file_name);
        let basic = ini.section("Basic");
        Self {
            source_ordinal,
            source,
            file_name: entry.file_name,
            display_name: entry.display_name,
            author: entry.author,
            briefing: entry.briefing,
            preview: entry.preview,
            multiplayer_start_waypoints: entry.multiplayer_start_waypoints,
            player_capacity: entry.player_capacity,
            preview_source_bounds: entry.preview_source_bounds,
            game_modes: parse_game_modes(ini),
            min_players: basic
                .and_then(|section| section.get_i32("MinPlayers"))
                .and_then(valid_player_count),
            max_players: basic
                .and_then(|section| section.get_i32("MaxPlayers"))
                .and_then(valid_player_count),
            official: basic
                .and_then(|section| section.get_bool("Official"))
                .unwrap_or(false),
            kind: SkirmishScenarioKind::ConcreteMap,
        }
    }

    /// Build a PKT-backed row.
    ///
    /// The map payload supplies only what the chooser draws from the map itself
    /// — preview, waypoints, capacity. Title, game-mode filter list and player
    /// bounds come from `entry`, i.e. the PKT's own `[<MultiMaps entry>]`
    /// section, because that is the INI and section the native record
    /// constructor is handed. Stock map payloads carry none of those keys, so
    /// reading them from the map is the same as reading nothing.
    pub fn pkt_from_ini(
        source_ordinal: usize,
        source: SkirmishScenarioSource,
        file_name: &str,
        ini: &IniFile,
        entry: PktEntryFields,
    ) -> Self {
        let mut record = Self::concrete_from_ini(source_ordinal, source, file_name, ini);
        record.display_name = entry.display_name;
        record.game_modes = entry.game_modes;
        record.min_players = Some(entry.min_players.unwrap_or(PKT_DEFAULT_MIN_PLAYERS));
        record.max_players = Some(entry.max_players.unwrap_or(PKT_DEFAULT_MAX_PLAYERS));
        // The constructor seeds the official flag true and never reads it back
        // for a PKT entry; only the loose-map header path reads `[Basic]`.
        record.official = true;
        record
    }

    pub fn random_map_sentinel(
        source_ordinal: usize,
        display_name: impl Into<String>,
        player_capacity: i32,
    ) -> Self {
        Self {
            source_ordinal,
            source: SkirmishScenarioSource::Synthetic,
            file_name: RANDMAP_SED.to_string(),
            display_name: display_name.into(),
            author: None,
            briefing: BriefingSection::default(),
            preview: PreviewSection::default(),
            multiplayer_start_waypoints: Vec::new(),
            player_capacity,
            preview_source_bounds: None,
            game_modes: Vec::new(),
            min_players: Some(RANDOM_MAP_MIN_PLAYERS),
            max_players: Some(RANDOM_MAP_MAX_PLAYERS),
            official: true,
            kind: SkirmishScenarioKind::RandomMapSentinel,
        }
    }

    pub fn from_map_menu_entry(source_ordinal: usize, entry: &MapMenuEntry) -> Self {
        Self {
            source_ordinal,
            source: source_for_file_name(&entry.file_name),
            file_name: entry.file_name.clone(),
            display_name: entry.display_name.clone(),
            author: entry.author.clone(),
            briefing: entry.briefing.clone(),
            preview: entry.preview.clone(),
            multiplayer_start_waypoints: entry.multiplayer_start_waypoints.clone(),
            player_capacity: entry.player_capacity,
            preview_source_bounds: entry.preview_source_bounds.clone(),
            game_modes: Vec::new(),
            min_players: None,
            max_players: None,
            official: false,
            kind: SkirmishScenarioKind::ConcreteMap,
        }
    }

    pub fn to_map_menu_entry(&self) -> MapMenuEntry {
        MapMenuEntry {
            file_name: self.file_name.clone(),
            display_name: self.display_name.clone(),
            author: self.author.clone(),
            briefing: self.briefing.clone(),
            preview: self.preview.clone(),
            multiplayer_start_waypoints: self.multiplayer_start_waypoints.clone(),
            player_capacity: self.player_capacity,
            preview_source_bounds: self.preview_source_bounds.clone(),
        }
    }
}

fn source_for_file_name(file_name: &str) -> SkirmishScenarioSource {
    let ext = std::path::Path::new(file_name)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("yro") => SkirmishScenarioSource::LooseYro(file_name.to_string()),
        Some("yrm") => SkirmishScenarioSource::LooseYrm(file_name.to_string()),
        Some("pkt") => SkirmishScenarioSource::LoosePkt(file_name.to_string()),
        _ => SkirmishScenarioSource::Synthetic,
    }
}

fn valid_player_count(value: i32) -> Option<u8> {
    (0..=u8::MAX as i32).contains(&value).then_some(value as u8)
}

/// Split one `GameMode=` value into filter tokens.
///
/// Retail writes the list with a space after each comma (`standard, meatgrind`),
/// and the native tokenizer trims each token from both ends against a
/// single-space charset, so the space is not significant.
///
/// Divergence, recorded not fixed: `str::trim` strips all Unicode whitespace
/// where native strips spaces only, so a custom PKT that separates its modes
/// with a tab would match here and not in gamemd. No stock entry uses anything
/// but a space.
pub fn parse_game_mode_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|mode| !mode.is_empty())
        .map(str::to_string)
        .collect()
}

/// Read the loose-map header's mode filter list. The key is `GameMode`,
/// singular — the only spelling the binary contains — read from `[Basic]` of
/// the map file itself. PKT-backed rows do not come through here.
pub fn parse_game_modes(ini: &IniFile) -> Vec<String> {
    ini.section("Basic")
        .and_then(|section| section.get("GameMode"))
        .map(parse_game_mode_list)
        .unwrap_or_default()
}

/// The record fields the PKT-entry constructor reads out of the PKT's own
/// `[<MultiMaps entry>]` section.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PktEntryFields {
    pub display_name: String,
    pub game_modes: Vec<String>,
    pub min_players: Option<u8>,
    pub max_players: Option<u8>,
}

pub fn filter_records_for_mode(
    records: &[SkirmishScenarioRecord],
    mode: &SkirmishGameMode,
) -> Vec<usize> {
    records
        .iter()
        .enumerate()
        .filter_map(|(idx, record)| record_matches_mode(record, mode).then_some(idx))
        .collect()
}

pub fn record_matches_mode(record: &SkirmishScenarioRecord, mode: &SkirmishGameMode) -> bool {
    match record.kind {
        SkirmishScenarioKind::RandomMapSentinel => mode.random_maps_allowed,
        SkirmishScenarioKind::ConcreteMap if record.game_modes.is_empty() => {
            mode.map_filter == "standard"
        }
        SkirmishScenarioKind::ConcreteMap => record
            .game_modes
            .iter()
            .any(|game_mode| game_mode == &mode.map_filter),
    }
}

/// Insert or refresh the single random-map row.
///
/// `player_capacity` comes from the seed's configured player count, which is
/// what decides how many setup slots the row offers — not a fixed quota.
pub fn upsert_random_map_sentinel(
    records: &mut Vec<SkirmishScenarioRecord>,
    display_name: impl Into<String>,
    player_capacity: i32,
) -> usize {
    if let Some(idx) = records
        .iter()
        .position(|record| record.kind == SkirmishScenarioKind::RandomMapSentinel)
    {
        records[idx].display_name = display_name.into();
        records[idx].file_name = RANDMAP_SED.to_string();
        records[idx].source = SkirmishScenarioSource::Synthetic;
        records[idx].multiplayer_start_waypoints.clear();
        records[idx].player_capacity = player_capacity;
        records[idx].preview_source_bounds = None;
        records[idx].game_modes.clear();
        records[idx].min_players = Some(RANDOM_MAP_MIN_PLAYERS);
        records[idx].max_players = Some(RANDOM_MAP_MAX_PLAYERS);
        records[idx].official = true;
        return idx;
    }

    let idx = records.len();
    records.push(SkirmishScenarioRecord::random_map_sentinel(
        idx,
        display_name,
        player_capacity,
    ));
    idx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skirmish_modes::stock_skirmish_modes;

    #[test]
    fn upserting_the_random_map_sentinel_keeps_exactly_one_row() {
        // Accepting Create Random Map twice must refresh the existing row, not
        // append a second one.
        let mut records = Vec::new();
        let first = upsert_random_map_sentinel(
            &mut records,
            "Random Map",
            i32::from(RANDOM_MAP_GENERATED_START_QUOTA),
        );
        let second = upsert_random_map_sentinel(
            &mut records,
            "Random Map",
            i32::from(RANDOM_MAP_GENERATED_START_QUOTA),
        );
        assert_eq!(first, second, "the sentinel is updated, never duplicated");

        let sentinels = records
            .iter()
            .filter(|record| record.kind == SkirmishScenarioKind::RandomMapSentinel)
            .count();
        assert_eq!(sentinels, 1);

        let sentinel = &records[first];
        assert_eq!(sentinel.file_name, RANDMAP_SED);
        assert!(sentinel.official, "the original constructs it official");
        // Current repo behaviour. The binary's record constructor passes 2 and 4
        // into +0x180/+0x184, but nothing reads those two fields when deciding a
        // player count - MPGameOptions__GetScenarioPlayerCount reads
        // [RandomMap] NumPlayers from the .SED instead - so the wider range here
        // is what the dialog's 2..8 trackbar actually produces.
        assert_eq!(
            (sentinel.min_players, sentinel.max_players),
            (Some(RANDOM_MAP_MIN_PLAYERS), Some(RANDOM_MAP_MAX_PLAYERS))
        );
    }

    fn mode(id: i32) -> SkirmishGameMode {
        stock_skirmish_modes()
            .into_iter()
            .find(|mode| mode.id == id)
            .expect("stock mode")
    }

    fn record(source_ordinal: usize, name: &str, game_modes: &str) -> SkirmishScenarioRecord {
        // Loose-map headers spell the key `GameMode`, singular.
        let ini = IniFile::from_str(&format!(
            "[Basic]\nName={name}\nGameMode={game_modes}\nMinPlayers=2\nMaxPlayers=8\nOfficial=yes\n\
             [Waypoints]\n0=100011\n1=110012\n"
        ));
        SkirmishScenarioRecord::concrete_from_ini(
            source_ordinal,
            SkirmishScenarioSource::LooseYrm(format!("{name}.yrm")),
            &format!("{name}.yrm"),
            &ini,
        )
    }

    #[test]
    fn scenario_record_parses_game_modes() {
        let rec = record(0, "Duel Map", "duel, meatgrind");
        assert_eq!(rec.game_modes, vec!["duel", "meatgrind"]);
        assert_eq!(rec.min_players, Some(2));
        assert_eq!(rec.max_players, Some(8));
        assert!(rec.official);
    }

    #[test]
    fn scenario_record_projects_to_map_menu_entry() {
        let rec = record(3, "Projected", "standard");
        let entry = rec.to_map_menu_entry();
        assert_eq!(entry.file_name, "Projected.yrm");
        assert_eq!(entry.display_name, "Projected");
        assert_eq!(entry.multiplayer_start_waypoints.len(), 2);
        assert_eq!(entry.player_capacity, 2);
    }

    #[test]
    fn pkt_record_uses_pkt_display_name_and_defaults() {
        // A stock map payload carries none of the record keys, so an entry that
        // supplies none of them falls back to the constructor's 2/4.
        let ini = IniFile::from_str("[Basic]\nName=Basic Name\n");
        let rec = SkirmishScenarioRecord::pkt_from_ini(
            7,
            SkirmishScenarioSource::MissionsMdPkt,
            "Official.MAP",
            &ini,
            PktEntryFields {
                display_name: "PKT Display".to_string(),
                ..PktEntryFields::default()
            },
        );

        assert_eq!(rec.source_ordinal, 7);
        assert_eq!(rec.display_name, "PKT Display");
        assert_eq!(rec.file_name, "Official.MAP");
        assert_eq!(rec.min_players, Some(PKT_DEFAULT_MIN_PLAYERS));
        assert_eq!(rec.max_players, Some(PKT_DEFAULT_MAX_PLAYERS));
        assert!(rec.official);
    }

    #[test]
    fn pkt_record_takes_mode_filter_and_bounds_from_the_entry_not_the_map() {
        // The map payload here declares the opposite of the PKT entry to prove
        // which file the record reads.
        let ini = IniFile::from_str(
            "[Basic]\nName=Basic Name\nGameMode=standard\nMinPlayers=6\nMaxPlayers=7\n",
        );
        let rec = SkirmishScenarioRecord::pkt_from_ini(
            0,
            SkirmishScenarioSource::MissionsMdPkt,
            "XMP02T2.MAP",
            &ini,
            PktEntryFields {
                display_name: "Dustbowl".to_string(),
                game_modes: parse_game_mode_list("standard, meatgrind"),
                min_players: Some(2),
                max_players: Some(2),
            },
        );

        assert_eq!(rec.game_modes, vec!["standard", "meatgrind"]);
        assert_eq!(rec.min_players, Some(2));
        assert_eq!(rec.max_players, Some(2));
        assert!(record_matches_mode(&rec, &mode(7)), "meatgrind must match");
        assert!(record_matches_mode(&rec, &mode(1)), "battle must match");
        assert!(!record_matches_mode(&rec, &mode(6)), "duel must not match");
    }

    #[test]
    fn pkt_entry_without_a_game_mode_key_matches_standard_only() {
        let ini = IniFile::from_str("[Basic]\nName=Nameless\n");
        let rec = SkirmishScenarioRecord::pkt_from_ini(
            0,
            SkirmishScenarioSource::MissionsMdPkt,
            "Nameless.MAP",
            &ini,
            PktEntryFields {
                display_name: "Nameless".to_string(),
                ..PktEntryFields::default()
            },
        );

        assert!(rec.game_modes.is_empty());
        assert!(record_matches_mode(&rec, &mode(1)));
        assert!(!record_matches_mode(&rec, &mode(9)));
    }

    #[test]
    fn choose_map_filters_by_selected_mpmode_game_modes() {
        let records = vec![
            record(0, "Battle", "standard"),
            record(1, "Team", "teamgame"),
            record(2, "Duel", "duel"),
        ];
        assert_eq!(filter_records_for_mode(&records, &mode(9)), vec![1]);
        assert_eq!(filter_records_for_mode(&records, &mode(6)), vec![2]);
    }

    #[test]
    fn choose_map_empty_game_modes_matches_standard_only() {
        let records = vec![record(0, "Empty", "")];
        assert_eq!(filter_records_for_mode(&records, &mode(1)), vec![0]);
        assert!(filter_records_for_mode(&records, &mode(9)).is_empty());
    }

    #[test]
    fn choose_map_filter_preserves_source_order_and_duplicates() {
        let records = vec![
            record(0, "Zoo", "standard"),
            record(1, "Alpha", "standard"),
            record(2, "Zoo", "standard"),
        ];
        let filtered = filter_records_for_mode(&records, &mode(1));
        assert_eq!(filtered, vec![0, 1, 2]);
        assert_eq!(records[0].display_name, records[2].display_name);
    }

    #[test]
    fn choose_map_filter_ignores_ui_label_and_category() {
        let battle = SkirmishGameMode {
            class: crate::skirmish_modes::MpModeClass::Battle,
            id: 42,
            ui_name_key: "GUI:TeamGame".to_string(),
            tooltip_key: String::new(),
            override_file: String::new(),
            map_filter: "standard".to_string(),
            random_maps_allowed: false,
            allies_allowed: true,
            must_ally: false,
        };
        let records = vec![record(0, "Standard", "standard")];
        assert_eq!(filter_records_for_mode(&records, &battle), vec![0]);
    }

    #[test]
    fn choose_map_filters_randmap_by_mode_random_allowed() {
        let records = vec![SkirmishScenarioRecord::random_map_sentinel(
            0,
            "Random Map",
            i32::from(RANDOM_MAP_GENERATED_START_QUOTA),
        )];
        assert_eq!(filter_records_for_mode(&records, &mode(1)), vec![0]);
        assert!(filter_records_for_mode(&records, &mode(9)).is_empty());
    }

    #[test]
    fn random_map_sentinel_spans_the_full_player_range() {
        // Pinned to literals on purpose: the capacity used to alias the
        // generated-start quota, which silently capped random maps at 4.
        assert_eq!(RANDOM_MAP_MIN_PLAYERS, 2);
        assert_eq!(RANDOM_MAP_MAX_PLAYERS, 8, "NumPlayers clamps to 2..8");
        assert_ne!(
            RANDOM_MAP_MAX_PLAYERS, RANDOM_MAP_GENERATED_START_QUOTA,
            "capacity must not be re-aliased to the start quota"
        );

        let rec = SkirmishScenarioRecord::random_map_sentinel(
            0,
            "Random Map",
            i32::from(RANDOM_MAP_GENERATED_START_QUOTA),
        );
        assert_eq!(rec.max_players, Some(8));
    }

    #[test]
    fn random_map_sentinel_advertises_shell_capacity_without_concrete_starts() {
        let rec = SkirmishScenarioRecord::random_map_sentinel(
            4,
            "Random Map",
            i32::from(RANDOM_MAP_GENERATED_START_QUOTA),
        );

        assert_eq!(rec.file_name, RANDMAP_SED);
        assert_eq!(rec.kind, SkirmishScenarioKind::RandomMapSentinel);
        assert_eq!(rec.min_players, Some(RANDOM_MAP_MIN_PLAYERS));
        assert_eq!(rec.max_players, Some(RANDOM_MAP_MAX_PLAYERS));
        assert!(rec.official);
        assert_eq!(
            rec.player_capacity,
            i32::from(RANDOM_MAP_GENERATED_START_QUOTA)
        );
        assert_eq!(RANDOM_MAP_GENERATED_START_QUOTA, 4);
        assert!(
            rec.multiplayer_start_waypoints.is_empty(),
            "the Choose Map sentinel is metadata only; generated maps must provide real waypoints before launch"
        );
    }

    #[test]
    fn skirmish_random_map_command_adds_or_updates_single_sentinel_record() {
        let mut records = vec![record(0, "Concrete", "standard")];
        let first = upsert_random_map_sentinel(
            &mut records,
            "Random Map",
            i32::from(RANDOM_MAP_GENERATED_START_QUOTA),
        );
        let second = upsert_random_map_sentinel(
            &mut records,
            "Updated Random Map",
            i32::from(RANDOM_MAP_GENERATED_START_QUOTA),
        );
        assert_eq!(first, 1);
        assert_eq!(second, 1);
        assert_eq!(records.len(), 2);
        assert_eq!(records[1].file_name, RANDMAP_SED);
        assert_eq!(records[1].display_name, "Updated Random Map");
        assert_eq!(records[1].min_players, Some(RANDOM_MAP_MIN_PLAYERS));
        assert_eq!(records[1].max_players, Some(RANDOM_MAP_MAX_PLAYERS));
        assert_eq!(
            records[1].player_capacity,
            i32::from(RANDOM_MAP_GENERATED_START_QUOTA)
        );
        assert!(records[1].official);
        assert!(records[1].multiplayer_start_waypoints.is_empty());
    }

    #[test]
    fn sentinel_capacity_follows_the_configured_player_count() {
        // The row's capacity is what decides how many setup slots the player
        // gets, so it has to track the seed's NumPlayers rather than a quota.
        let mut records = Vec::new();
        upsert_random_map_sentinel(&mut records, "Random Map", 6);
        assert_eq!(records[0].player_capacity, 6);
        assert_eq!(records[0].to_map_menu_entry().player_capacity, 6);

        upsert_random_map_sentinel(&mut records, "Random Map", 3);
        assert_eq!(records.len(), 1, "still one row");
        assert_eq!(records[0].player_capacity, 3, "a re-accept updates it");
    }

    #[test]
    fn the_sentinel_map_entry_is_addressable_by_its_seed_file_name() {
        // Committing a chooser selection resolves it against the loadable map
        // list by file name; a mismatch here is what makes an accepted random
        // map unplayable.
        let mut records = Vec::new();
        let idx = upsert_random_map_sentinel(&mut records, "Random Map", 2);
        let entry = records[idx].to_map_menu_entry();
        assert!(entry.file_name.eq_ignore_ascii_case(RANDMAP_SED));
    }

    #[test]
    fn random_map_upsert_repairs_stale_sentinel_metadata() {
        let mut stale = SkirmishScenarioRecord::random_map_sentinel(
            0,
            "Old Random Map",
            i32::from(RANDOM_MAP_GENERATED_START_QUOTA),
        );
        stale.file_name = "Wrong.yrm".to_string();
        stale.source = SkirmishScenarioSource::LooseYrm("Wrong.yrm".to_string());
        stale.multiplayer_start_waypoints = vec![Waypoint {
            index: 0,
            rx: 11,
            ry: 100,
        }];
        stale.min_players = None;
        stale.max_players = None;
        stale.official = false;

        let mut records = vec![stale];
        let idx = upsert_random_map_sentinel(
            &mut records,
            "Random Map",
            i32::from(RANDOM_MAP_GENERATED_START_QUOTA),
        );

        assert_eq!(idx, 0);
        assert_eq!(records[0].file_name, RANDMAP_SED);
        assert_eq!(records[0].source, SkirmishScenarioSource::Synthetic);
        assert_eq!(records[0].min_players, Some(RANDOM_MAP_MIN_PLAYERS));
        assert_eq!(records[0].max_players, Some(RANDOM_MAP_MAX_PLAYERS));
        assert!(records[0].official);
        assert!(
            records[0].multiplayer_start_waypoints.is_empty(),
            "upsert must not preserve fabricated starts on the sentinel"
        );
    }

    #[test]
    fn scenario_records_retain_explicit_source_ordinals() {
        let records = vec![
            record(10, "From Missions", "standard"),
            record(11, "From Loose Pkt", "standard"),
            record(12, "From Yro", "standard"),
            record(13, "From Yrm", "standard"),
        ];
        let ordinals: Vec<usize> = records.iter().map(|record| record.source_ordinal).collect();
        assert_eq!(ordinals, vec![10, 11, 12, 13]);
    }
}
