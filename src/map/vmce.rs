//! VMCE series manifest and local map discovery.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

use super::{
    houses::{HouseRoster, parse_house_roster},
    source,
    triggers::SCENARIO_GLOBAL_COUNT,
};
use crate::assets::csf_file::CsfFile;
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT;
use crate::util::file_system as fs;

const MANIFEST_FILE: &str = "campaign.ini";
const MANIFEST_MAX_BYTES: usize = 1024 * 1024;
const SERIES_CSF_MAX_BYTES: usize = 8 * 1024 * 1024;
const SERIES_CREDITS_FILE: &str = "credits.txt";
const SERIES_CREDITS_MAX_BYTES: usize = 32 * 1024;
const TITLE_MAX_CHARS: usize = 128;

#[derive(Debug)]
pub struct VmceManifest {
    pub name_label: Option<String>,
    pub stages: Vec<VmceStage>,
    pub entry_index: usize,
    carry_over: BTreeMap<(usize, usize), Vec<(String, String)>>,
    ini: IniFile,
}

#[derive(Debug)]
pub struct VmceStage {
    pub path: String,
    pub primary_roles: Vec<String>,
    options: StageOverrides,
    next_scenario: Option<String>,
    alt_next_scenario: Option<String>,
    alt_next_global: usize,
    end_of_game: bool,
}

#[derive(Debug, Default)]
struct StageOverrides {
    min_players: Option<usize>,
    max_players: Option<usize>,
    allow_allies_change: Option<bool>,
    allow_observers: Option<bool>,
    auto_enemy_selection: Option<bool>,
    lobby_difficulty: Option<i32>,
    lock_difficulty: Option<bool>,
}

struct StageHouseTerms {
    shared_control_houses: Vec<Vec<String>>,
    role_camera_waypoints: BTreeMap<String, u32>,
}

impl StageOverrides {
    fn read(section: &IniSection, lower: &Self) -> Result<Self> {
        Ok(Self {
            min_players: read_player_limit(section, "MinPlayers")?.or(lower.min_players),
            max_players: read_player_limit(section, "MaxPlayers")?.or(lower.max_players),
            allow_allies_change: read_bool(section, "AllowAlliesChange")?
                .or(lower.allow_allies_change),
            allow_observers: read_bool(section, "AllowObservers")?.or(lower.allow_observers),
            auto_enemy_selection: read_bool(section, "AutoEnemySelection")?
                .or(lower.auto_enemy_selection),
            lobby_difficulty: read_difficulty(section)?.or(lower.lobby_difficulty),
            lock_difficulty: read_bool(section, "LockDifficulty")?.or(lower.lock_difficulty),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VmceStageOptions {
    pub min_players: usize,
    pub max_players: usize,
    pub allow_allies_change: bool,
    pub allow_observers: bool,
    #[serde(default)]
    pub auto_enemy_selection: bool,
    /// Easiest-first, the same order as the map's trigger flags.
    pub lobby_difficulty: i32,
    pub lock_difficulty: bool,
}

impl VmceManifest {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= MANIFEST_MAX_BYTES,
            "manifest exceeds {MANIFEST_MAX_BYTES} bytes"
        );
        let ini = {
            // campaign.ini has its own grammar. Preserve physical duplicates
            // for validation even when the game's map and rules use Ares syntax.
            let _stock = crate::rules::ares_switch::read_ares_way(false);
            IniFile::from_bytes(bytes)?
        };
        let series = ini.section("Campaign").context("missing [Campaign]")?;
        let sections = ini.section_names();
        ensure!(
            sections
                .iter()
                .filter(|name| name.eq_ignore_ascii_case("Campaign"))
                .count()
                == 1,
            "duplicate [Campaign] section"
        );
        let mut stage_sections = HashMap::new();
        for name in sections {
            let Some((kind, map)) = name.split_once(':') else {
                continue;
            };
            if kind.eq_ignore_ascii_case("Scenario") && !map.contains(':') {
                let key = map.replace('\\', "/").to_ascii_lowercase();
                stage_sections
                    .entry(key)
                    .and_modify(|section| *section = None)
                    .or_insert(Some(name));
            }
        }
        let entry = map_path(required_name(series, "Entry")?).context("Entry")?;
        let count = read_count(series, "ScenarioCount", series.entry_count())?;
        check_distinct_keys(
            series,
            &["entry", "name", "scenariocount", "altnextglobal"],
            (1..=count).map(|index| format!("scenario{index}")),
        )
        .context("[Campaign]")?;
        let series_options =
            StageOverrides::read(series, &StageOverrides::default()).context("[Campaign]")?;
        // The native GameExit::Victory at 0x00685A38..0x00685A5C uses
        // global 1 to choose the alternate path. A series can override that index.
        let series_alt_global = read_alt_next_global(series)
            .context("[Campaign]")?
            .unwrap_or(1);
        let mut stages = Vec::with_capacity(count);
        let mut paths = HashSet::new();
        for index in 1..=count {
            let key = format!("Scenario{index}");
            let raw_path = required_name(series, &key)?;
            let path = map_path(raw_path).with_context(|| key.clone())?;
            let canonical_path = path.to_ascii_lowercase();
            ensure!(
                paths.insert(canonical_path.clone()),
                "[Campaign] {key}: duplicate scenario path {path}"
            );
            let section_name = stage_sections
                .get(&canonical_path)
                .with_context(|| format!("missing [Scenario:{raw_path}]"))?
                .with_context(|| format!("duplicate scenario section [Scenario:{raw_path}]"))?;
            let section = ini
                .section(section_name)
                .with_context(|| format!("missing [{section_name}]"))?;
            let primary_roles = read_roles(section).with_context(|| format!("[{section_name}]"))?;
            check_distinct_keys(
                section,
                &[
                    "playercount",
                    "endofgame",
                    "nextscenario",
                    "altnextscenario",
                    "altnextglobal",
                ],
                (1..=primary_roles.len()).map(|index| format!("player{index}")),
            )
            .with_context(|| format!("[{section_name}]"))?;
            let options = StageOverrides::read(section, &series_options)
                .with_context(|| format!("[{section_name}]"))?;
            let end_of_game = read_bool(section, "EndOfGame")
                .with_context(|| format!("[{section_name}]"))?
                .unwrap_or(false);
            let continuation = |key: &str| {
                if end_of_game {
                    return Ok(None);
                }
                section
                    .read_name(key, usize::MAX)
                    .map(map_path)
                    .transpose()
                    .with_context(|| format!("[{section_name}] {key}"))
            };
            let alt_next_global = read_alt_next_global(section)
                .with_context(|| format!("[{section_name}]"))?
                .unwrap_or(series_alt_global);
            stages.push(VmceStage {
                path,
                primary_roles,
                options,
                next_scenario: continuation("NextScenario")?,
                alt_next_scenario: continuation("AltNextScenario")?,
                alt_next_global,
                end_of_game,
            });
        }
        for stage in &stages {
            for (key, next) in [
                ("NextScenario", stage.next_scenario.as_deref()),
                ("AltNextScenario", stage.alt_next_scenario.as_deref()),
            ] {
                let Some(next) = next else { continue };
                ensure!(
                    paths.contains(&next.to_ascii_lowercase()),
                    "{}: {key} {next} is absent from the scenario list",
                    stage.path
                );
            }
        }
        let entry_index = stages
            .iter()
            .position(|stage| stage.path.eq_ignore_ascii_case(&entry))
            .with_context(|| format!("Entry {entry} is absent from the scenario list"))?;
        let carry_over = read_carry_over_sections(&ini, &stages)?;
        Ok(Self {
            name_label: series.read_name("Name", usize::MAX).map(str::to_owned),
            stages,
            entry_index,
            carry_over,
            ini,
        })
    }

    fn stage_index(&self, path: &str) -> Result<usize> {
        self.stages
            .iter()
            .position(|stage| stage.path == path)
            .with_context(|| format!("{path} is absent from the scenario list"))
    }

    fn successor(&self, index: usize, globals: &[bool; SCENARIO_GLOBAL_COUNT]) -> Option<usize> {
        let current = self.stages.get(index)?;
        if current.end_of_game {
            return None;
        }
        let next = current
            .alt_next_scenario
            .as_deref()
            .filter(|_| globals.get(current.alt_next_global) == Some(&true))
            .or(current.next_scenario.as_deref());
        match next {
            Some(next) => self
                .stages
                .iter()
                .position(|stage| stage.path.eq_ignore_ascii_case(next)),
            None => self.stages.get(index + 1).map(|_| index + 1),
        }
    }
}

fn read_carry_over_sections(
    ini: &IniFile,
    stages: &[VmceStage],
) -> Result<BTreeMap<(usize, usize), Vec<(String, String)>>> {
    let mut carry_over = BTreeMap::new();
    for name in ini.section_names() {
        let Some((kind, pair)) = name.split_once(':') else {
            continue;
        };
        if !kind.eq_ignore_ascii_case("CarryOver") {
            continue;
        }
        let (from, to) = pair
            .split_once(':')
            .with_context(|| format!("[{name}] requires source and destination stages"))?;
        let resolve = |raw| {
            let path = map_path(raw).with_context(|| format!("[{name}]"))?;
            stages
                .iter()
                .position(|stage| stage.path.eq_ignore_ascii_case(&path))
                .with_context(|| format!("[{name}] references unknown stage {path}"))
        };
        let (from, to) = (resolve(from)?, resolve(to)?);
        let roles = carry_over_roles(ini.section_or_empty(name), &stages[from], &stages[to])?;
        ensure!(
            carry_over.insert((from, to), roles).is_none(),
            "duplicate carry-over section [{name}]"
        );
    }
    Ok(carry_over)
}

fn carry_over_roles(
    section: &IniSection,
    from: &VmceStage,
    to: &VmceStage,
) -> Result<Vec<(String, String)>> {
    let mut source_roles = HashSet::new();
    let mut target_roles = HashSet::new();
    section
        .physical_keys()
        .map(|source| {
            let target = required_name(section, source)?;
            let source = from
                .primary_roles
                .iter()
                .find(|role| role.eq_ignore_ascii_case(source))
                .with_context(|| format!("[{}] unknown source role {source}", section.name))?;
            let target = to
                .primary_roles
                .iter()
                .find(|role| role.eq_ignore_ascii_case(target))
                .with_context(|| format!("[{}] unknown destination role {target}", section.name))?;
            ensure!(
                source_roles.insert(source) && target_roles.insert(target),
                "[{}] carry-over role mapping must be one to one",
                section.name
            );
            Ok((source.clone(), target.clone()))
        })
        .collect()
}

impl VmceStage {
    fn primary_houses(&self, roster: &HouseRoster) -> Result<Vec<String>> {
        self.primary_roles
            .iter()
            .map(|role| {
                roster
                    .houses
                    .iter()
                    .find(|house| house.name.eq_ignore_ascii_case(role))
                    .map(|house| house.name.clone())
                    .with_context(|| format!("primary role House {role} is absent from [Houses]"))
            })
            .collect()
    }

    /// Resolve per-House manifest settings against the admitted map roster.
    fn house_terms(&self, ini: &IniFile, roster: &HouseRoster) -> Result<StageHouseTerms> {
        let control_keys: Vec<_> = (1..=self.primary_roles.len())
            .map(|role| format!("Player{role}Control"))
            .collect();
        let mut seen_houses = HashSet::new();
        let mut granted_houses = vec![Vec::new(); self.primary_roles.len()];
        let mut role_camera_waypoints = BTreeMap::new();
        for section_name in ini.section_names() {
            let Some((kind, rest)) = section_name.split_once(':') else {
                continue;
            };
            let Some((path, target)) = rest.split_once(':') else {
                continue;
            };
            if !kind.eq_ignore_ascii_case("Scenario")
                || !path.replace('\\', "/").eq_ignore_ascii_case(&self.path)
            {
                continue;
            }
            let house = roster
                .houses
                .iter()
                .find(|house| house.name.eq_ignore_ascii_case(target))
                .with_context(|| {
                    format!("[{section_name}] House {target} is absent from [Houses]")
                })?;
            ensure!(
                seen_houses.insert(house.name.as_str()),
                "[{section_name}] duplicate control section for House {}",
                house.name
            );
            let mut seen_roles = HashSet::new();
            let section = ini.section_or_empty(section_name);
            let is_primary = self
                .primary_roles
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&house.name));
            if is_primary && let Some(waypoint) = read_camera_waypoint(section) {
                role_camera_waypoints.insert(house.name.clone(), waypoint);
            }
            for key in section.physical_keys() {
                let lower = key.to_ascii_lowercase();
                if !lower.starts_with("player") || !lower.ends_with("control") {
                    continue;
                }
                let role = control_keys
                    .iter()
                    .position(|expected| expected.eq_ignore_ascii_case(key))
                    .with_context(|| format!("[{section_name}] {key} refers to an invalid role"))?;
                ensure!(
                    seen_roles.insert(role),
                    "[{section_name}] duplicate control setting for {}",
                    control_keys[role]
                );
                if read_bool(section, key)
                    .with_context(|| format!("[{section_name}]"))?
                    .unwrap_or(false)
                {
                    ensure!(
                        !is_primary,
                        "[{section_name}] primary role House {} cannot be shared",
                        house.name
                    );
                    granted_houses[role].push(house.name.clone());
                }
            }
        }
        for houses in &mut granted_houses {
            houses.sort_unstable();
        }
        Ok(StageHouseTerms {
            shared_control_houses: granted_houses,
            role_camera_waypoints,
        })
    }
}

fn read_camera_waypoint(section: &IniSection) -> Option<u32> {
    let raw = section.read_name("CameraWaypoint", usize::MAX)?;
    // Reject malformed values instead of letting ReadInt's atoi fallback
    // turn them into a request for waypoint 0.
    if raw.trim_start_matches('0').len() > 3 || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    crate::map::waypoints::waypoint_slot(section.read_int("CameraWaypoint", -1))
}

fn required_name<'a>(section: &'a IniSection, key: &str) -> Result<&'a str> {
    section
        .read_name(key, usize::MAX)
        .with_context(|| format!("missing {key}"))
}

fn check_distinct_keys(
    section: &IniSection,
    fields: &[&str],
    numbered: impl Iterator<Item = String>,
) -> Result<()> {
    const ROOM_OPTIONS: &[&str] = &[
        "minplayers",
        "maxplayers",
        "allowallieschange",
        "allowobservers",
        "autoenemyselection",
        "difficulty",
        "lockdifficulty",
    ];
    let numbered = numbered.collect::<HashSet<_>>();
    let mut keys = HashSet::new();
    for key in section.physical_keys() {
        let normalized = key.to_ascii_lowercase();
        let name = normalized.as_str();
        if ROOM_OPTIONS.contains(&name) || fields.contains(&name) || numbered.contains(&normalized)
        {
            ensure!(keys.insert(normalized), "duplicate key {key}");
        } else {
            log::debug!("VMCE ignoring [{}] key {key}", section.name);
        }
    }
    Ok(())
}

fn read_count(section: &IniSection, key: &str, max: usize) -> Result<usize> {
    let value = required_name(section, key)?;
    let count = usize::try_from(section.read_int(key, 0)).unwrap_or(0);
    // The native reader is permissive. A manifest count must account for
    // every declared row, including when the native integer would wrap.
    ensure!(
        (1..=max).contains(&count) && value.trim_start_matches('0') == count.to_string(),
        "{key} must be a decimal count between 1 and {max}"
    );
    Ok(count)
}

fn read_player_limit(section: &IniSection, key: &str) -> Result<Option<usize>> {
    section
        .is_present(key)
        .then(|| read_count(section, key, NETWORK_PLAYER_SLOT_COUNT))
        .transpose()
}

fn read_bool(section: &IniSection, key: &str) -> Result<Option<bool>> {
    section
        .is_present(key)
        .then(|| {
            section
                .read_bool_value(key)
                .with_context(|| format!("{key} must be a boolean"))
        })
        .transpose()
}

fn read_difficulty(section: &IniSection) -> Result<Option<i32>> {
    if !section.is_present("Difficulty") {
        return Ok(None);
    }
    let text = required_name(section, "Difficulty")?;
    let value = section.read_int("Difficulty", -1);
    ensure!(
        (0..=2).contains(&value) && text == value.to_string(),
        "Difficulty must be 0 (Easy), 1 (Normal), or 2 (Hard)"
    );
    Ok(Some(value))
}

fn read_alt_next_global(section: &IniSection) -> Result<Option<usize>> {
    const KEY: &str = "AltNextGlobal";
    if !section.is_present(KEY) {
        return Ok(None);
    }
    let text = required_name(section, KEY)?;
    let index = section.read_int(KEY, -1);
    let max_index = SCENARIO_GLOBAL_COUNT as i32 - 1;
    ensure!(
        (0..=max_index).contains(&index) && text == index.to_string(),
        "{KEY} must be a decimal global index between 0 and {max_index}"
    );
    Ok(Some(index as usize))
}

fn read_roles(section: &IniSection) -> Result<Vec<String>> {
    let count = read_count(section, "PlayerCount", NETWORK_PLAYER_SLOT_COUNT)?;
    let mut houses = HashSet::new();
    let mut roles = Vec::with_capacity(count);
    for index in 1..=count {
        let key = format!("Player{index}");
        let name = required_name(section, &key)?;
        ensure!(
            houses.insert(name.to_ascii_lowercase()),
            "{key}: duplicate primary role House {name}"
        );
        roles.push(name.to_owned());
    }
    Ok(roles)
}

fn map_path(name: &str) -> Result<String> {
    source::validate_network_map_name(name)?;
    let path = name.replace('\\', "/");
    ensure!(
        path.split('/').all(|part| !part.ends_with(['.', ' ']))
            && !path.contains(['<', '>', '"', '|', '?', '*'])
            && source::is_loose_map_name(Path::new(&path)),
        "invalid scenario path {name}"
    );
    Ok(path)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VmceMapId {
    pub series: String,
    pub stage: String,
}

impl VmceMapId {
    pub fn validate(&self) -> Result<()> {
        validate_series_name(&self.series)?;
        ensure!(
            map_path(&self.stage)? == self.stage,
            "noncanonical scenario path"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredVmceMap {
    pub id: VmceMapId,
    pub path: PathBuf,
    pub name: String,
    pub is_entry: bool,
    pub stage_number: usize,
    pub min_players: usize,
    pub max_players: usize,
    series_directory: PathBuf,
    series_label: Option<String>,
    series_title: Option<String>,
    stage_title: Option<String>,
}

#[derive(Debug)]
pub struct OpenedVmceStage {
    pub record: DiscoveredVmceMap,
    pub map: source::LoadedMapIni,
    pub manifest_sha256: String,
    pub options: VmceStageOptions,
    pub primary_houses: Vec<String>,
    pub house_names: Vec<String>,
    pub shared_control_houses: Vec<Vec<String>>,
    role_camera_waypoints: BTreeMap<String, u32>,
    briefing_text: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum VmceStageFileError {
    #[error("The VMCE series manifest campaign.ini is missing")]
    MissingManifest,
    #[error("The selected VMCE map is missing")]
    MissingMap,
    #[error("The VMCE series manifest campaign.ini differs from the host's file")]
    ManifestMismatch,
    #[error("The selected VMCE map differs from the host's file")]
    MapMismatch,
}

impl DiscoveredVmceMap {
    pub fn with_series_titles(mut self) -> Self {
        let labels = series_csf(&self.series_directory);
        self.use_series_titles(labels.as_ref());
        self
    }

    fn use_series_titles(&mut self, labels: Option<&CsfFile>) {
        self.series_title = self
            .series_label
            .as_deref()
            .and_then(|label| csf_name(labels, label))
            .map(str::to_owned);
        self.stage_title = csf_name(labels, &self.name).map(str::to_owned);
    }

    pub fn display_name(&self, base: Option<&CsfFile>) -> String {
        let series = self.series_display_title(base);
        let stage = self.stage_display_title(base).unwrap_or(&self.id.stage);
        format!("{series} / {stage}")
    }

    pub fn series_display_title<'a>(&'a self, base: Option<&'a CsfFile>) -> &'a str {
        self.series_title
            .as_deref()
            .or_else(|| csf_name(base, self.series_label.as_deref()?))
            .unwrap_or(&self.id.series)
    }

    pub fn stage_display_title<'a>(&'a self, base: Option<&'a CsfFile>) -> Option<&'a str> {
        self.stage_title
            .as_deref()
            .or_else(|| csf_name(base, &self.name))
            .or_else(|| {
                let name = self.name.trim();
                (!name.is_empty()
                    && !name.eq_ignore_ascii_case(&self.id.stage)
                    && !is_csf_label(name)
                    && !name.chars().any(char::is_control))
                .then_some(name)
            })
    }
}

impl OpenedVmceStage {
    /// Resolve House UIName, Briefing and ranking labels against the selected
    /// series' string table.
    pub fn presentation(
        &self,
        base: Option<&CsfFile>,
        stage_fallback: String,
    ) -> crate::map::scenario_menu::LocalScenarioPresentation {
        self.presentation_from_map(base, stage_fallback, &self.map.ini)
    }

    pub fn presentation_from_map(
        &self,
        base: Option<&CsfFile>,
        stage_fallback: String,
        map: &IniFile,
    ) -> crate::map::scenario_menu::LocalScenarioPresentation {
        let table = series_csf(&self.record.series_directory);
        let resolve = |key: &str| {
            csf_name(table.as_ref(), key)
                .or_else(|| csf_name(base, key))
                .map(str::to_owned)
        };
        let mut record = self.record.clone();
        record.use_series_titles(table.as_ref());
        let briefing = self.briefing_text.clone().or_else(|| {
            let key = map.section("Basic")?.read_string("Briefing", "", 128);
            let text = table
                .as_ref()
                .and_then(|table| table.get(&key))
                .or_else(|| base.and_then(|base| base.get(&key)))?;
            let text: String = text
                .chars()
                .filter(|c| !c.is_control() || c.is_whitespace())
                .take(crate::map::basic::MAX_BRIEFING_CHARS)
                .collect();
            (!text.trim().is_empty()).then_some(text)
        });
        let report_strings = if map.section("Ranking").is_some() {
            let ranking = crate::map::ranking::parse_ranking_section(map);
            [
                ranking.under_par_title,
                ranking.under_par_message,
                ranking.over_par_title,
                ranking.over_par_message,
            ]
            .into_iter()
            .filter_map(|key| {
                let text = table.as_ref()?.get(&key)?.to_owned();
                Some((key, text))
            })
            .collect()
        } else {
            Default::default()
        };
        let role_titles = self
            .primary_houses
            .iter()
            .map(|house| {
                let key = map.section(house)?.read_name("UIName", 32)?;
                resolve(key)
            })
            .collect();
        let mut placed = self
            .primary_houses
            .iter()
            .map(|house| (house.as_str(), (0, 0)))
            .collect::<std::collections::BTreeMap<_, (usize, usize)>>();
        let new_ini_format = crate::map::basic::parse_basic_section(map).new_ini_format;
        for entity in crate::map::entities::parse_map_entities(map, new_ini_format.unwrap_or(0)) {
            if let Some((units, buildings)) = placed.get_mut(entity.owner.as_str()) {
                if entity.category == crate::map::entities::EntityCategory::Structure {
                    *buildings += 1;
                } else {
                    *units += 1;
                }
            }
        }
        let role_appearances = self
            .primary_houses
            .iter()
            .zip(&self.shared_control_houses)
            .map(|(name, shared)| {
                let house = map.section_or_empty(name);
                let shared_forces = shared
                    .iter()
                    .map(|name| {
                        let house = map.section_or_empty(name);
                        house
                            .read_name("UIName", 32)
                            .and_then(&resolve)
                            .or_else(|| {
                                house.read_name("Country", 128).and_then(|country| {
                                    map.section(country)?
                                        .read_name("UIName", 32)
                                        .and_then(&resolve)
                                })
                            })
                    })
                    .collect();
                let country = house.read_name("Country", 128);
                let (units, buildings) = placed.get(name.as_str()).copied().unwrap_or_default();
                crate::map::scenario_menu::LocalScenarioRoleAppearance {
                    country: country.map(str::to_owned),
                    color: house.read_name("Color", 128).map(str::to_owned),
                    opening: crate::map::scenario_menu::LocalScenarioRoleOpening {
                        funds: crate::map::houses::scenario_funds(house.read_int("Credits", 0)),
                        tech_level: house.read_int_value("TechLevel"),
                        units,
                        buildings,
                    },
                    shared_forces,
                    resolved_visual: None,
                }
            })
            .collect();
        crate::map::scenario_menu::LocalScenarioPresentation {
            series_title: record.series_display_title(base).to_owned(),
            stage_title: record
                .stage_display_title(base)
                .map_or(stage_fallback, str::to_owned),
            briefing,
            report_strings,
            role_titles,
            role_camera_waypoints: self.role_camera_waypoints.clone(),
            role_appearances,
            preview: crate::map::preview::decode_preview_image_from_ini(map)
                .ok()
                .flatten(),
            starts: crate::map::scenario_menu::preview_source_bounds_from_verified_source(map)
                .filter(|bounds| bounds.start_points.len() == self.primary_houses.len()),
        }
    }
}

fn csf_name<'a>(table: Option<&'a CsfFile>, label: &str) -> Option<&'a str> {
    let title = table?.get(label)?.trim();
    (!title.is_empty()
        && title.chars().take(TITLE_MAX_CHARS + 1).count() <= TITLE_MAX_CHARS
        && !title.chars().any(char::is_control))
    .then_some(title)
}

fn is_csf_label(name: &str) -> bool {
    name.split_once(':').is_some_and(|(prefix, suffix)| {
        (prefix.eq_ignore_ascii_case("VMCE") || prefix.eq_ignore_ascii_case("Name"))
            && !suffix.is_empty()
            && !suffix.starts_with(char::is_whitespace)
    })
}

#[derive(Debug)]
pub struct VmceDiscoveryError {
    pub path: PathBuf,
    pub reason: anyhow::Error,
}

#[derive(Debug, Default)]
pub struct VmceDiscovery {
    pub maps: Vec<DiscoveredVmceMap>,
    pub unavailable: Vec<DiscoveredVmceMap>,
    pub errors: Vec<VmceDiscoveryError>,
}

impl VmceDiscovery {
    fn report(&mut self, path: PathBuf, reason: anyhow::Error) {
        self.errors.push(VmceDiscoveryError { path, reason });
    }
}

/// Discover the locally available stages independently. A missing or invalid
/// stage does not hide the other playable files in its series.
pub fn discover(maps_root: &Path) -> VmceDiscovery {
    let mut found = VmceDiscovery::default();
    let root = match vmce_root(maps_root) {
        Ok(root) => root,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == ErrorKind::NotFound) =>
        {
            return found;
        }
        Err(error) => {
            found.report(maps_root.join("VMCE"), error);
            return found;
        }
    };
    match fs::read_dir(&root) {
        Ok(entries) => {
            for entry in entries {
                match entry {
                    Ok(entry) => {
                        let path = entry.path();
                        if let Err(reason) = discover_series(&root, &path, &mut found) {
                            found.report(path, reason);
                        }
                    }
                    Err(error) => found.report(root.clone(), error.into()),
                }
            }
        }
        Err(reason) => found.report(root, reason.into()),
    }
    let stage_order = |a: &DiscoveredVmceMap, b: &DiscoveredVmceMap| {
        a.id.series
            .cmp(&b.id.series)
            .then_with(|| b.is_entry.cmp(&a.is_entry))
            .then_with(|| a.stage_number.cmp(&b.stage_number))
    };
    found.maps.sort_by(stage_order);
    found.unavailable.sort_by(stage_order);
    found
}

/// Re-read just the chosen series and map. Chooser rows are discovery results;
/// files removed or edited while the chooser was open must be checked again.
pub fn open_stage(maps_root: &Path, id: &VmceMapId) -> Result<OpenedVmceStage> {
    open_stage_with_identities(maps_root, id, None)
}

/// Check the exact selected inputs before parsing them. Both hashes refer
/// to the very buffers consumed by the manifest and map readers.
pub fn open_stage_with_identities(
    maps_root: &Path,
    id: &VmceMapId,
    expected: Option<(&str, &str)>,
) -> Result<OpenedVmceStage> {
    let (series, manifest, manifest_sha256) =
        read_series(maps_root, id, expected.map(|(manifest, _)| manifest))?;
    let index = manifest.stage_index(&id.stage)?;
    read_stage(
        &series,
        &id.series,
        &manifest,
        &manifest_sha256,
        index,
        expected.map(|(_, map)| map),
    )
    .with_context(|| series.join(&id.stage).display().to_string())
    .map_err(|error| report_missing(error, VmceStageFileError::MissingMap))
}

/// The manifest's continuation once a stage ends in victory.
#[derive(Debug, PartialEq, Eq)]
pub enum VmceContinuation {
    Next {
        id: VmceMapId,
        funds: Vec<(String, String)>,
    },
    Credits(Option<String>),
    SameStage,
}

/// Resolve a victory from the manifest used at start. The room still
/// validates the suggested map through normal selection when it reopens.
pub fn stage_continuation(
    maps_root: &Path,
    current: &VmceMapId,
    configuration_sha256: &str,
    terminal_globals: &[bool; SCENARIO_GLOBAL_COUNT],
) -> Result<VmceContinuation> {
    let (series, manifest, _) = read_series(maps_root, current, Some(configuration_sha256))?;
    let index = manifest.stage_index(&current.stage)?;
    Ok(
        if manifest
            .stages
            .get(index)
            .is_some_and(|stage| stage.end_of_game)
        {
            VmceContinuation::Credits(
                read_series_credits(&series)
                    .inspect_err(|error| {
                        log::warn!(
                            "VMCE {}: {error:#}",
                            series.join(SERIES_CREDITS_FILE).display()
                        )
                    })
                    .unwrap_or_default(),
            )
        } else if let Some(next) = manifest.successor(index, terminal_globals) {
            VmceContinuation::Next {
                id: VmceMapId {
                    series: current.series.clone(),
                    stage: manifest.stages[next].path.clone(),
                },
                funds: manifest
                    .carry_over
                    .get(&(index, next))
                    .cloned()
                    .unwrap_or_default(),
            }
        } else {
            VmceContinuation::SameStage
        },
    )
}

fn read_series_credits(series: &Path) -> Result<Option<String>> {
    let Some(candidate) = crate::util::case_insensitive_path::find(series, SERIES_CREDITS_FILE)
    else {
        return Ok(None);
    };
    let path = contained_file(series, &candidate)?;
    let bytes = read_bounded_file(&path, SERIES_CREDITS_MAX_BYTES)?;
    let text = String::from_utf8(bytes).context("credits.txt must be UTF-8")?;
    let text = text.trim_start_matches('\u{feff}');
    ensure!(
        text.chars()
            .all(|c| !c.is_control() || matches!(c, '\r' | '\n' | '\t')),
        "credits.txt contains a control character"
    );
    Ok((!text.trim().is_empty()).then(|| text.to_owned()))
}

fn read_series(
    maps_root: &Path,
    id: &VmceMapId,
    expected_sha256: Option<&str>,
) -> Result<(PathBuf, VmceManifest, String)> {
    id.validate()?;
    (|| -> Result<_> {
        let root = vmce_root(maps_root)?;
        let series = contained_path(&root, &root.join(&id.series))?;
        let (manifest, sha256) = read_manifest(&series, expected_sha256)
            .with_context(|| series.join(MANIFEST_FILE).display().to_string())?;
        Ok((series, manifest, sha256))
    })()
    .map_err(|error| report_missing(error, VmceStageFileError::MissingManifest))
}

fn report_missing(error: anyhow::Error, missing: VmceStageFileError) -> anyhow::Error {
    if error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|io| io.kind() == ErrorKind::NotFound)
    {
        error.context(missing)
    } else {
        error
    }
}

fn vmce_root(maps_root: &Path) -> Result<PathBuf> {
    let maps = fs::canonicalize(maps_root)?;
    contained_path(&maps, &maps.join("VMCE"))
}

fn validate_series_name(series: &str) -> Result<()> {
    source::validate_network_map_name(series)?;
    ensure!(
        !series.contains(['/', '\\', '<', '>', '"', '|', '?', '*'])
            && !series.ends_with(['.', ' ']),
        "invalid series directory name"
    );
    Ok(())
}

fn read_manifest(series: &Path, expected_sha256: Option<&str>) -> Result<(VmceManifest, String)> {
    let path = contained_file(series, &series.join(MANIFEST_FILE))?;
    let bytes = read_bounded_file(&path, MANIFEST_MAX_BYTES)?;
    let sha256 = crate::util::sha256::sha256_hex(&bytes);
    ensure!(
        expected_sha256.is_none_or(|expected| expected == sha256),
        VmceStageFileError::ManifestMismatch
    );
    let manifest = VmceManifest::parse(&bytes)?;
    Ok((manifest, sha256))
}

fn read_stage(
    series_directory: &Path,
    series: &str,
    manifest: &VmceManifest,
    manifest_sha256: &str,
    index: usize,
    expected_sha256: Option<&str>,
) -> Result<OpenedVmceStage> {
    let stage = &manifest.stages[index];
    let path = contained_file(series_directory, &series_directory.join(&stage.path))?;
    let (map, briefing_text) = source::read_loose_map_ini(
        &path,
        |source| {
            ensure!(
                expected_sha256.is_none_or(|expected| {
                    source
                        .physical_identity()
                        .is_some_and(|(_, sha256)| sha256 == expected)
                }),
                VmceStageFileError::MapMismatch
            );
            Ok(())
        },
        crate::map::basic::briefing_text,
    )
    .context("could not read map INI")?;
    let roster = parse_house_roster(&map.ini, &[], None);
    let primary_houses = stage.primary_houses(&roster)?;
    let StageHouseTerms {
        shared_control_houses,
        role_camera_waypoints,
    } = stage.house_terms(&manifest.ini, &roster)?;
    let basic = map.ini.section_or_empty("Basic");
    let min_players = match stage.options.min_players {
        Some(minimum) => minimum,
        None => read_player_limit(basic, "MinPlayers")
            .context("[Basic]")?
            .unwrap_or(1),
    };
    let max_players = match stage.options.max_players {
        Some(maximum) => maximum,
        None => read_player_limit(basic, "MaxPlayers")
            .context("[Basic]")?
            .unwrap_or(primary_houses.len()),
    };
    ensure!(
        max_players <= primary_houses.len(),
        "MaxPlayers exceeds PlayerCount"
    );
    ensure!(min_players <= max_players, "MinPlayers exceeds MaxPlayers");
    let name = basic.read_string("Name", &stage.path, TITLE_MAX_CHARS);
    let options = VmceStageOptions {
        min_players,
        max_players,
        allow_allies_change: stage
            .options
            .allow_allies_change
            .or(read_bool(basic, "AllowAlliesChange")?)
            .unwrap_or(false),
        allow_observers: stage
            .options
            .allow_observers
            .or(read_bool(basic, "AllowObservers")?)
            .unwrap_or(false),
        auto_enemy_selection: stage
            .options
            .auto_enemy_selection
            .or(read_bool(basic, "AutoEnemySelection")?)
            .unwrap_or(false),
        lobby_difficulty: stage
            .options
            .lobby_difficulty
            .or(read_difficulty(basic).context("[Basic]")?)
            .unwrap_or(1),
        lock_difficulty: stage
            .options
            .lock_difficulty
            .or(read_bool(basic, "LockDifficulty")?)
            .unwrap_or(false),
    };
    Ok(OpenedVmceStage {
        record: DiscoveredVmceMap {
            id: VmceMapId {
                series: series.to_owned(),
                stage: stage.path.clone(),
            },
            path,
            series_directory: series_directory.to_owned(),
            series_label: manifest.name_label.clone(),
            series_title: None,
            stage_title: None,
            name,
            is_entry: index == manifest.entry_index,
            stage_number: index + 1,
            min_players,
            max_players,
        },
        map,
        manifest_sha256: manifest_sha256.to_owned(),
        options,
        primary_houses,
        house_names: roster.houses.into_iter().map(|house| house.name).collect(),
        shared_control_houses,
        role_camera_waypoints,
        briefing_text,
    })
}

fn discover_series(root: &Path, path: &Path, found: &mut VmceDiscovery) -> Result<()> {
    let resolved = contained_path(root, path)?;
    if !fs::is_dir(&resolved) {
        return Ok(());
    }
    let series = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("series directory needs a Unicode name")?;
    validate_series_name(series)?;

    let manifest_path = path.join(MANIFEST_FILE);
    let (manifest, manifest_sha256) = match read_manifest(&resolved, None) {
        Ok(manifest) => manifest,
        Err(reason) => {
            found.report(manifest_path, reason);
            return Ok(());
        }
    };
    let labels = series_csf(&resolved);
    for (index, stage) in manifest.stages.iter().enumerate() {
        match read_stage(&resolved, series, &manifest, &manifest_sha256, index, None) {
            Ok(mut map) => {
                map.record.use_series_titles(labels.as_ref());
                found.maps.push(map.record);
            }
            Err(reason) => {
                let path = resolved.join(&stage.path);
                let mut record = DiscoveredVmceMap {
                    id: VmceMapId {
                        series: series.to_owned(),
                        stage: stage.path.clone(),
                    },
                    path: path.clone(),
                    name: String::new(),
                    is_entry: index == manifest.entry_index,
                    stage_number: index + 1,
                    min_players: stage.options.min_players.unwrap_or(1),
                    max_players: stage
                        .options
                        .max_players
                        .unwrap_or(stage.primary_roles.len()),
                    series_directory: resolved.clone(),
                    series_label: manifest.name_label.clone(),
                    series_title: None,
                    stage_title: None,
                };
                record.use_series_titles(labels.as_ref());
                found.unavailable.push(record);
                found.report(path, reason);
            }
        }
    }
    Ok(())
}

fn series_csf(series: &Path) -> Option<CsfFile> {
    let candidate = crate::util::case_insensitive_path::find(series, "ra2md.csf")?;
    let load = || -> Result<_> {
        let path = contained_file(series, &candidate)?;
        Ok(CsfFile::from_bytes(&read_bounded_file(
            &path,
            SERIES_CSF_MAX_BYTES,
        )?)?)
    };
    load()
        .inspect_err(|error| log::warn!("VMCE {}: {error:#}", candidate.display()))
        .ok()
}

fn read_bounded_file(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    let bytes = {
        use std::io::Read;

        let mut bytes = Vec::new();
        fs::File::open(path)?
            .take(maximum as u64 + 1)
            .read_to_end(&mut bytes)?;
        bytes
    };
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    let bytes = fs::read_shared(path)?;
    ensure!(bytes.len() <= maximum, "file exceeds {maximum} bytes");
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    let bytes = bytes.to_vec();
    Ok(bytes)
}

fn contained_path(root: &Path, path: &Path) -> Result<PathBuf> {
    let resolved = fs::canonicalize(path)?;
    ensure!(
        resolved != root && resolved.starts_with(root),
        "path resolves outside {}",
        root.display()
    );
    Ok(resolved)
}

fn contained_file(root: &Path, path: &Path) -> Result<PathBuf> {
    let resolved = contained_path(root, path)?;
    ensure!(fs::is_file(&resolved), "expected a regular file");
    Ok(resolved)
}

#[cfg(all(test, tests_map))]
mod tests {
    use super::*;
    use crate::map::source::test_support::TestDirectory;

    fn fixture_manifest() -> &'static str {
        crate::test_fixture::text("tests/fixtures/vmce/campaign.ini")
    }

    fn fixture_map() -> &'static [u8] {
        crate::test_fixture::bytes("tests/fixtures/vmce/minimal_stage01.map")
    }

    #[test]
    fn manifest_reads_entry_and_requires_the_declared_houses() {
        let manifest = VmceManifest::parse(fixture_manifest().as_bytes()).unwrap();
        let stage = &manifest.stages[manifest.entry_index];
        let roster = parse_house_roster(&IniFile::from_bytes(fixture_map()).unwrap(), &[], None);
        assert_eq!(stage.path, "minimal_stage01.map");
        assert_eq!(stage.primary_roles, ["HouseAlpha"]);
        assert_eq!(stage.primary_houses(&roster).unwrap(), ["HouseAlpha"]);

        let invalid = fixture_manifest().replace("Player1=HouseAlpha", "Player1=MissingHouse");
        let manifest = VmceManifest::parse(invalid.as_bytes()).unwrap();
        let error = manifest.stages[0].primary_houses(&roster).unwrap_err();
        assert!(error.to_string().contains("MissingHouse"));
    }

    #[test]
    fn opening_camera_waypoints_are_scoped_to_the_primary_house() {
        let manifest = fixture_manifest();
        let roster = parse_house_roster(&IniFile::from_bytes(fixture_map()).unwrap(), &[], None);
        let projection = |value: &str| {
            let text = format!(
                "{manifest}\n[scenario:MINIMAL_STAGE01.MAP:housealpha]\nCameraWaypoint={value}\n"
            );
            let manifest = VmceManifest::parse(text.as_bytes()).unwrap();
            manifest.stages[manifest.entry_index]
                .house_terms(&manifest.ini, &roster)
                .unwrap()
                .role_camera_waypoints
        };
        assert_eq!(projection("0"), BTreeMap::from([("HouseAlpha".into(), 0)]));
        assert!(projection("invalid").is_empty());
        assert!(projection("702").is_empty());
    }

    #[test]
    fn shared_move_control_is_scoped_to_the_current_stage_and_role() {
        let root = TestDirectory::new("vmce-shared-authority");
        let id = VmceMapId {
            series: "Shared".into(),
            stage: "shared_move_stage01.map".into(),
        };
        let manifest_path = "VMCE/Shared/campaign.ini";
        let manifest = crate::test_fixture::text("tests/fixtures/vmce/shared_move/campaign.ini");
        fs::create_dir_all(&root.path().join("VMCE/Shared")).unwrap();
        root.write(
            "VMCE/Shared/shared_move_stage01.map",
            crate::test_fixture::bytes("tests/fixtures/vmce/shared_move/shared_move_stage01.map"),
        );
        let selected_grants = |text: &str| {
            root.write(manifest_path, text.as_bytes());
            open_stage(root.path(), &id)
                .map(|stage| stage.shared_control_houses.into_iter().next().unwrap())
        };
        let other_stage = format!("{manifest}\n[Scenario:later.map:Missing]\nPlayer8Control=bad\n");
        assert_eq!(selected_grants(&other_stage).unwrap(), ["HouseBravo"]);
        assert!(
            selected_grants(&manifest.replace("Player1Control=yes", "Player1Control=no"))
                .unwrap()
                .is_empty()
        );
        let other_role = manifest
            .replace("PlayerCount=1", "PlayerCount=2\nPlayer2=HouseCharlie")
            .replace("Player1Control=yes", "Player2Control=yes");
        assert!(selected_grants(&other_role).unwrap().is_empty());
        for (text, expected) in [
            (
                manifest.replace("HouseBravo]", "MissingHouse]"),
                "absent from [Houses]",
            ),
            (
                manifest.replace("Player1Control", "Player2Control"),
                "invalid role",
            ),
            (
                manifest.replace("HouseBravo]", "HouseAlpha]"),
                "primary role House HouseAlpha cannot be shared",
            ),
        ] {
            let error = selected_grants(&text).unwrap_err();
            assert!(format!("{error:#}").contains(expected), "{error:#}");
        }
    }

    #[test]
    fn manifest_rejects_an_unlisted_entry_repeated_houses_and_unsafe_paths() {
        let manifest = fixture_manifest();
        for (text, expected) in [
            (
                fixture_manifest().replace("Entry=minimal_stage01.map", "Entry=missing.map"),
                "absent from the scenario list",
            ),
            (
                format!("{manifest}\nNextScenario=missing.map\n"),
                "NextScenario missing.map is absent from the scenario list",
            ),
            (
                format!("{manifest}\nAltNextScenario=missing.map\n"),
                "AltNextScenario missing.map is absent from the scenario list",
            ),
            (
                format!("{manifest}\nAltNextGlobal={SCENARIO_GLOBAL_COUNT}\n"),
                "AltNextGlobal must be a decimal global index",
            ),
            (
                fixture_manifest().replace("PlayerCount=1", "PlayerCount=2\nPlayer2=housealpha"),
                "duplicate primary role",
            ),
            (
                fixture_manifest().replace("minimal_stage01.map", "../minimal_stage01.map"),
                "invalid logical multiplayer map name",
            ),
            (
                fixture_manifest().replace("minimal_stage01.map", "C:\\minimal_stage01.map"),
                "invalid logical multiplayer map name",
            ),
            (
                fixture_manifest().replace(
                    "ScenarioCount=1",
                    "ScenarioCount=2\nScenario2=MINIMAL_STAGE01.MAP",
                ),
                "duplicate scenario path",
            ),
        ] {
            let error = VmceManifest::parse(text.as_bytes()).unwrap_err();
            assert!(format!("{error:#}").contains(expected), "{error:#}");
        }
    }

    #[test]
    fn victory_successor_uses_the_accepted_manifest_without_opening_maps() {
        let root = TestDirectory::new("vmce-next-stage");
        let path = "VMCE/Outcomes/campaign.ini";
        let original = crate::test_fixture::text("tests/fixtures/vmce/lan_outcomes/campaign.ini");
        fs::create_dir_all(&root.path().join("VMCE/Outcomes")).unwrap();
        let write = |text: &str| {
            root.write(path, text.as_bytes());
            crate::util::sha256::sha256_hex(text.as_bytes())
        };
        let stage = |file| VmceMapId {
            series: "Outcomes".into(),
            stage: file,
        };
        let first = stage("outcome_stage01.map".into());
        let second = stage("outcome_stage02.map".into());
        let globals = [false; SCENARIO_GLOBAL_COUNT];
        let next = |id: &VmceMapId, sha: &str| stage_continuation(root.path(), id, sha, &globals);
        let accepted = write(original);
        assert_eq!(
            next(&first, &accepted).unwrap(),
            VmceContinuation::Next {
                id: second.clone(),
                funds: Vec::new(),
            }
        );
        assert_eq!(
            next(&second, &accepted).unwrap(),
            VmceContinuation::SameStage
        );
        let mapped = format!(
            "{original}\n[CarryOver:OUTCOME_STAGE01.MAP:outcome_stage02.map]\nhousealpha=housecharlie\n",
        );
        assert_eq!(
            next(&first, &write(&mapped)).unwrap(),
            VmceContinuation::Next {
                id: second.clone(),
                funds: vec![("HouseAlpha".into(), "HouseCharlie".into())],
            }
        );
        let ambiguous = format!("{mapped}HouseCharlie=HouseCharlie\n");
        let error = next(&first, &write(&ambiguous)).unwrap_err();
        assert!(format!("{error:#}").contains("one to one"), "{error:#}");

        let changed = original.replace(
            "[Scenario:outcome_stage02.map]",
            "[Scenario:outcome_stage02.map]\nNextScenario=OUTCOME_STAGE01.MAP",
        );
        let changed_sha = write(&changed);
        let error = next(&first, &accepted).unwrap_err();
        assert!(matches!(
            error.downcast_ref::<VmceStageFileError>(),
            Some(VmceStageFileError::ManifestMismatch)
        ));
        assert_eq!(
            next(&second, &changed_sha).unwrap(),
            VmceContinuation::Next {
                id: first,
                funds: Vec::new(),
            }
        );
        let ended = write(&changed.replace("NextScenario=", "EndOfGame=yes\nNextScenario="));
        assert_eq!(
            next(&second, &ended).unwrap(),
            VmceContinuation::Credits(None)
        );
        let credits = "系列制作组\r\n\r\n感谢游玩";
        root.write(
            "VMCE/Outcomes/CREDITS.TXT",
            format!("\u{feff}{credits}").as_bytes(),
        );
        assert_eq!(
            next(&second, &ended).unwrap(),
            VmceContinuation::Credits(Some(credits.to_owned()))
        );
        root.write("VMCE/Outcomes/CREDITS.TXT", &[0xff]);
        assert_eq!(
            next(&second, &ended).unwrap(),
            VmceContinuation::Credits(None)
        );
    }

    #[test]
    fn victory_branch_uses_the_effective_global_and_yields_to_end_of_game() {
        let original = fixture_manifest().replace(
            "ScenarioCount=1",
            "ScenarioCount=3\nScenario2=continue.map\nScenario3=alternate.map",
        ) + "\nAltNextScenario=ALTERNATE.MAP\n\
            [Scenario:continue.map]\nPlayerCount=1\nPlayer1=HouseAlpha\n\
            [Scenario:alternate.map]\nPlayerCount=1\nPlayer1=HouseAlpha\n";
        let successor = |text: &str, globals: &[bool; SCENARIO_GLOBAL_COUNT]| {
            let manifest = VmceManifest::parse(text.as_bytes()).unwrap();
            manifest
                .successor(manifest.entry_index, globals)
                .map(|index| manifest.stages[index].path.clone())
        };
        let mut globals = [false; SCENARIO_GLOBAL_COUNT];
        assert_eq!(
            successor(&original, &globals).as_deref(),
            Some("continue.map")
        );
        globals[1] = true;
        assert_eq!(
            successor(&original, &globals).as_deref(),
            Some("alternate.map")
        );

        let series = original.replace("[Campaign]", "[Campaign]\nAltNextGlobal=49");
        assert_eq!(
            successor(&series, &globals).as_deref(),
            Some("continue.map")
        );
        globals[49] = true;
        assert_eq!(
            successor(&series, &globals).as_deref(),
            Some("alternate.map")
        );

        let stage = series.replace("AltNextScenario=", "AltNextGlobal=0\nAltNextScenario=");
        assert_eq!(successor(&stage, &globals).as_deref(), Some("continue.map"));
        globals[0] = true;
        assert_eq!(
            successor(&stage, &globals).as_deref(),
            Some("alternate.map")
        );
        let terminal = stage.replace("AltNextScenario=", "EndOfGame=yes\nAltNextScenario=");
        assert_eq!(successor(&terminal, &globals), None);
    }

    #[test]
    fn discovery_keeps_valid_stages_and_reports_failed_paths() {
        let root = TestDirectory::new("vmce");
        let good = "VMCE/Good";
        let broken = "VMCE/Broken";
        for directory in [good, broken] {
            std::fs::create_dir_all(root.path().join(directory)).unwrap();
        }
        let manifest = fixture_manifest().replace(
            "ScenarioCount=1",
            "ScenarioCount=2\nScenario2=stage02/missing.map",
        ) + "\n[Scenario:stage02/missing.map]\nPlayerCount=1\nPlayer1=HouseAlpha\n";
        root.write(&format!("{good}/{MANIFEST_FILE}"), manifest.as_bytes());
        root.write(&format!("{good}/minimal_stage01.map"), fixture_map());
        root.write(&format!("{broken}/{MANIFEST_FILE}"), b"[Campaign]\n");

        let found = discover(root.path());
        assert_eq!(found.maps.len(), 1, "{found:#?}");
        assert_eq!(
            found.maps[0].id,
            VmceMapId {
                series: "Good".to_owned(),
                stage: "minimal_stage01.map".to_owned()
            }
        );
        assert!(found.maps[0].is_entry);
        assert_eq!(found.errors.len(), 2, "{found:#?}");
        for suffix in ["Broken/campaign.ini", "Good/stage02/missing.map"] {
            assert!(
                found
                    .errors
                    .iter()
                    .any(|error| error.path.ends_with(suffix))
            );
        }
        let id = &found.maps[0].id;
        assert_eq!(open_stage(root.path(), id).unwrap().record, found.maps[0]);
        root.write(
            &format!("{good}/minimal_stage01.map"),
            b"[Basic]\nName=Edited\n",
        );
        let error = open_stage(root.path(), id).unwrap_err();
        assert!(format!("{error:#}").contains("HouseAlpha"), "{error:#}");
    }

    #[test]
    fn installed_series_titles_use_series_csf_then_base_then_local_paths() {
        use crate::assets::csf_file::{build_test_csf, test_csf};
        let root = TestDirectory::new("vmce-stage-labels");
        let directory = "VMCE/Local";
        fs::create_dir_all(root.path().join(directory)).unwrap();
        let manifest = fixture_manifest().replace("[Campaign]", "[Campaign]\nName=VMCE:SERIES");
        let map = String::from_utf8(fixture_map().to_vec()).unwrap();
        let basic_name = map.lines().find(|line| line.starts_with("Name=")).unwrap();
        let map = map.replacen(basic_name, "Name=VMCE:STAGE", 1);
        root.write(&format!("{directory}/{MANIFEST_FILE}"), manifest.as_bytes());
        root.write(&format!("{directory}/minimal_stage01.map"), map.as_bytes());
        let csf = format!("{directory}/RA2MD.CSF");
        root.write(&csf, &build_test_csf("VMCE:SERIES", "战役"));
        let base = test_csf(&[("VMCE:SERIES", "Base series"), ("VMCE:STAGE", "Base stage")]);

        let map = discover(root.path()).maps.remove(0);
        assert_eq!(map.display_name(Some(&base)), "战役 / Base stage");
        let plain = open_stage(root.path(), &map.id).unwrap().record;
        assert_eq!(plain.display_name(Some(&base)), "Base series / Base stage");
        assert_eq!(
            plain.with_series_titles().display_name(Some(&base)),
            "战役 / Base stage"
        );

        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(root.path().join(&csf))
            .unwrap();
        file.set_len(SERIES_CSF_MAX_BYTES as u64 + 1).unwrap();
        drop(file);
        assert_eq!(
            discover(root.path()).maps[0].display_name(Some(&base)),
            "Base series / Base stage"
        );
        root.write(&csf, b"truncated");
        let map = open_stage(root.path(), &map.id)
            .unwrap()
            .record
            .with_series_titles();
        assert_eq!(map.display_name(Some(&base)), "Base series / Base stage");
        assert_eq!(map.display_name(None), "Local / minimal_stage01.map");
        let mut literal = map;
        literal.name = "Mission:Breakout".to_owned();
        assert_eq!(literal.display_name(None), "Local / Mission:Breakout");
    }

    #[test]
    fn ambiguous_manifest_sections_and_impossible_player_limits_are_refused() {
        let manifest = fixture_manifest();
        let duplicate = format!("{manifest}\n[scenario:MINIMAL_STAGE01.MAP]\nPlayerCount=1\n");
        let error = VmceManifest::parse(duplicate.as_bytes()).unwrap_err();
        assert!(format!("{error:#}").contains("duplicate scenario section"));

        let duplicate =
            fixture_manifest().replace("Player1=HouseAlpha", "Player1=HouseAlpha\nPlayer1=Other");
        let _ares = crate::rules::ares_switch::read_ares_way(true);
        let error = VmceManifest::parse(duplicate.as_bytes()).unwrap_err();
        assert!(format!("{error:#}").contains("duplicate key Player1"));
        let unused = format!(
            "{manifest}Author=Example\nauthor=Other\n[Scenario:unused.map]\nMinPlayers=1\n\
             [scenario:UNUSED.MAP]\nMinPlayers=2\n"
        );
        assert_eq!(
            VmceManifest::parse(unused.as_bytes()).unwrap().stages.len(),
            1
        );

        let root = TestDirectory::new("vmce-player-limits");
        fs::create_dir_all(root.path().join("VMCE/Impossible")).unwrap();
        let manifest = fixture_manifest().replace("[Campaign]", "[Campaign]\nMinPlayers=2");
        root.write("VMCE/Impossible/campaign.ini", manifest.as_bytes());
        root.write("VMCE/Impossible/minimal_stage01.map", fixture_map());
        let found = discover(root.path());
        assert!(found.maps.is_empty());
        assert_eq!(found.errors.len(), 1);
        assert!(format!("{:#}", found.errors[0].reason).contains("MinPlayers exceeds MaxPlayers"));
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn discovery_refuses_a_link_to_a_map_outside_the_series() {
        let root = TestDirectory::new("vmce-link");
        let series = root.path().join("VMCE/Linked");
        std::fs::create_dir_all(&series).unwrap();
        root.write("outside.map", fixture_map());
        let link = series.join("outside");
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.path(), &link).unwrap();
        // A directory junction exercises Windows path resolution without
        // requiring elevated permission to create file symlinks.
        #[cfg(windows)]
        {
            let result = std::process::Command::new("powershell.exe")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "New-Item -ItemType Junction -Path $env:VMCE_TEST_LINK -Target $env:VMCE_TEST_TARGET -ErrorAction Stop | Out-Null",
                ])
                .env("VMCE_TEST_LINK", &link)
                .env("VMCE_TEST_TARGET", root.path())
                .output()
                .unwrap();
            assert!(result.status.success(), "{:?}", result);
        }
        let manifest = fixture_manifest().replace("minimal_stage01.map", "outside/outside.map");
        root.write("VMCE/Linked/campaign.ini", manifest.as_bytes());

        let found = discover(root.path());
        #[cfg(unix)]
        std::fs::remove_file(&link).unwrap();
        #[cfg(windows)]
        std::fs::remove_dir(&link).unwrap();
        assert!(found.maps.is_empty(), "{found:#?}");
        assert_eq!(found.errors.len(), 1, "{found:#?}");
        assert!(found.errors[0].path.ends_with("outside/outside.map"));
        assert!(
            found.errors[0]
                .reason
                .to_string()
                .contains("resolves outside")
        );
    }
}
