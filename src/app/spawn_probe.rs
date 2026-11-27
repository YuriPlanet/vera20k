//! Launch compatibility probe for external launchers (docs/spawn.md, "Probe").
//!
//! `vera20k --probe` (desktop) and the browser's `probe()` report what a
//! spawn.ini launch on this installation will be checked against: the build
//! version and multiplayer rules digest the multiplayer agreement compares,
//! the client map's physical SHA-256 and effective digest, and the country
//! and colour indices spawn.ini fields address, and the campaigns a
//! single-player launch may start. Every value comes from the
//! owner the match itself uses: [`agreement::version`], the process
//! [`ProcessAssets`] rules digest for the loaded default Skirmish mode, and
//! [`crate::map::source::load_network_map_ini`] for the map, so two
//! installations whose probes agree pass each other's agreement.
//!
//! It runs headless: no window, audio output, network or profile writes.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::app::frontend::cncnet_spawn::{max_spawn_color_index, max_spawn_country_index};
use crate::app::process_assets::ProcessAssets;
use crate::app::rules_parts::RulesParts;
use crate::app::shell_network_setup::agreement;
use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::assets::csf_file::CsfFile;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ProbeGameMode {
    pub id: i32,
    pub rules_digest: String,
    /// `rules_digest` in parts (docs/spawn.md, "Rules parts").
    pub rules_parts: RulesParts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ProbeReport {
    /// The multiplayer agreement's version: package version and network
    /// build hash. `ExpectedVersion`.
    pub version: String,
    pub build_identity: BuildIdentity,
    /// The multiplayer rules agreement for the loaded default Skirmish mode:
    /// eligible id 1, else the first eligible row. `ExpectedRulesDigest`.
    pub rules_digest: String,
    /// `rules_digest` in parts, which say what differs when two games'
    /// digests do (docs/spawn.md, "Rules parts"). An older build's report
    /// lacks the field.
    pub rules_parts: RulesParts,
    /// `GameMode=` values and each one's agreement: the digest covers the mode's
    /// INI, so `ExpectedRulesDigest` must come from the row the launch selects.
    pub game_modes: Vec<ProbeGameMode>,
    pub map: Option<ProbeMap>,
    /// `Side=` values: Country array indices this build admits, read from the
    /// loaded rules (a mod's own `[Countries]` when one is chosen).
    pub countries: Vec<ProbeCountry>,
    /// `Color=` values: colour priorities and the `[Colors]` entry each draws.
    pub colors: Vec<IndexedName>,
    /// Start waypoints the map authors (`[SpawnLocations]` 0..N-1); null
    /// without `map`.
    pub max_start_waypoints: Option<i32>,
    /// The campaign catalog (`[Battles]` of the BATTLEMD file the INI roots
    /// name), in list order.
    pub campaigns: Vec<ProbeCampaign>,
    /// This build makes random-map files: `--generate-map` on the desktop,
    /// `generate_map()` in the browser (docs/spawn.md, "Random maps"). An
    /// older build's report lacks the field, and that build would start the
    /// game for `--generate-map` instead.
    pub generate_map: bool,
    /// The most houses a launch with one human and no `[Relay]` seats
    /// (`SKIRMISH_MAX_SLOT_COUNT`); every other launch seats at most thirty.
    /// An older build's report lacks the field, and that build seats thirty.
    pub max_offline_houses: u8,
    /// This build reads players who share an explicit `[SpawnLocations]`
    /// start as one House (docs/spawn.md, "Shared starts"). An older build's
    /// report lacks the field, and that build refuses a shared start.
    pub shared_starts: bool,
}

/// One `[Battles]` campaign of the game data, as the campaign startup reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ProbeCampaign {
    pub id: String,
    /// The first mission's file name: spawn.ini `Scenario` with `IsSinglePlayer`.
    pub scenario: String,
    /// That mission's `UIName` in the MISSIONMD file the INI roots name,
    /// through the game's string table; empty when the mission has no name.
    pub name: String,
    /// `Description=` through the string table, or as written when it names
    /// no label; empty when absent.
    pub description: String,
    /// `DebugOnly=yes`: the retail campaign menu does not offer it.
    pub debug_only: bool,
    /// The missions a won campaign passes through, as victory continues it:
    /// `scenario`, then the choice in the MAPSELMD file the INI roots name
    /// after each mission, until a mission is `OneTimeOnly`/`EndOfGame` or
    /// names no next stage.
    pub missions: Vec<ProbeMission>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ProbeMission {
    pub scenario: String,
    /// As [`ProbeCampaign::name`].
    pub name: String,
}

/// More than any campaign chain; bounds a malformed MAPSELMD loop.
const MAX_CAMPAIGN_MISSIONS: usize = 64;

/// The parts of [`ProbeReport::version`], for display; clients may play
/// together when `version` and `rules_digest` (and the map) are equal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct BuildIdentity {
    pub package_version: String,
    /// Hash of the multiplayer sources and enabled Cargo features.
    pub network_build: String,
    /// Informational: the extension features this build has, which
    /// `network_build` covers.
    pub phobos: bool,
    pub npatch: bool,
    pub verax: bool,
    /// Informational: the build's platform.
    pub target: &'static str,
    /// Informational: the commit this build was made from, as its builder
    /// named it (`VERA20K_COMMIT`); null when none was named.
    pub commit: Option<String>,
}

/// `vera20k --version` and the browser's `version()` (docs/spawn.md,
/// "Version"): the probe's `version` and `build_identity`, which need no game
/// folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct VersionReport {
    pub version: String,
    pub build_identity: BuildIdentity,
}

pub(crate) fn version_report() -> VersionReport {
    VersionReport {
        version: agreement::version(),
        build_identity: build_identity(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ProbeMap {
    /// Logical name every participant shares (the file name).
    pub file: String,
    pub byte_length: usize,
    /// SHA-256 of the physical map bytes. `ExpectedMapDigest`.
    pub source_sha256: String,
    /// Digest of the expanded map content the network agreement compares.
    pub effective_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct IndexedName {
    pub index: u32,
    pub name: String,
}

/// One `Side=` value and what the skirmish country menu shows for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ProbeCountry {
    /// The Country array index `Side=` takes.
    pub index: u32,
    /// The Country's ID as `[Countries]` lists it.
    pub name: String,
    /// Its `UIName=` through the game's string table, as the menu draws it
    /// (`country_label`); empty when the table has no such label.
    pub label: String,
    /// Its `[Sides]` index after any `Side=` override; null when unbound.
    pub side: Option<u8>,
    /// Whether the skirmish menu offers it (`country_choices`, 4E3A00):
    /// `Multiplay=yes`, a `UIName=`, and an Ares `ListIndex` of 0 or more.
    pub selectable: bool,
}

/// The physical identity and start capacity of a launcher's map, read
/// exactly as an external network launch loads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpawnMapIdentity {
    pub map: ProbeMap,
    pub start_waypoints: i32,
}

pub(crate) fn spawn_map_identity(
    root: &Path,
    path: &str,
    assets: &AssetManager,
) -> Result<SpawnMapIdentity> {
    let loaded = crate::map::source::load_network_map_ini(root, path, assets)
        .with_context(|| format!("cannot load map {path}"))?;
    let (byte_length, source_sha256) = loaded
        .source
        .physical_identity()
        .context("map has no physical source")?;
    let file = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .context("map has no file name")?
        .to_owned();
    Ok(SpawnMapIdentity {
        map: ProbeMap {
            file,
            byte_length,
            source_sha256: source_sha256.to_owned(),
            effective_digest: crate::map::source::effective_network_map_digest(
                &loaded.ini,
                false,
                false,
            ),
        },
        start_waypoints: crate::map::waypoints::skirmish_player_capacity(&loaded.ini),
    })
}

pub(crate) fn build_identity() -> BuildIdentity {
    BuildIdentity {
        package_version: env!("CARGO_PKG_VERSION").to_owned(),
        network_build: env!("VERA20K_NETWORK_BUILD").to_owned(),
        phobos: crate::rules::PHOBOS_ENABLED,
        // Every build carries NPatch; the game's data decides whether it plays.
        npatch: true,
        verax: crate::rules::VERAX_ENABLED,
        target: if cfg!(all(target_arch = "wasm32", target_os = "unknown")) {
            "wasm32"
        } else {
            std::env::consts::OS
        },
        commit: Some(env!("VERA20K_BUILD_COMMIT"))
            .filter(|commit| !commit.is_empty())
            .map(str::to_owned),
    }
}

/// The process sources startup loads (config INI roots, personal rules,
/// neutral archives) from the game folder and the selected `active_mod`, with the
/// startup Rules and shell projection their first load returns
/// (`ProcessAssets::initialize_sources_if_needed`). `load_audio_index` reads
/// the audio bag's index too, as startup does; it costs a hash of the bag.
pub(crate) fn load_process_sources(
    config: &crate::util::config::GameConfig,
    personal_rules: crate::rules::personal_rules::PersonalRules,
    media_archive_mode: MediaArchiveMode,
    active_mod: Option<crate::app::mods::ActiveMod>,
    ares_setting: crate::util::ares_launch::AresSetting,
    kratos_setting: crate::util::kratos_launch::KratosSetting,
    load_audio_index: bool,
) -> Result<(
    ProcessAssets,
    Option<crate::rules::ruleset::RuleSet>,
    Option<crate::rules::ini_parser::IniFile>,
)> {
    let (resources, ares_setting, kratos_setting) = crate::app::mods::resources_for(
        active_mod.as_ref(),
        &config.ini,
        ares_setting,
        kratos_setting,
    )
    .map_err(anyhow::Error::msg)?;
    let mut assets = AssetManager::with_mod_and_ares(
        &config.paths.ra2_dir,
        resources.mod_folder.as_deref(),
        media_archive_mode,
        ares_setting,
        &resources.archives,
    )
    .and_then(|assets| assets.with_resource_config(&resources))
    .context("could not load the game archives")?;
    assets.set_kratos_setting(kratos_setting);
    let mut process = ProcessAssets::new(media_archive_mode, load_audio_index)
        .with_resources(resources)
        .with_mod_name(active_mod.map(|active| active.folder))
        .with_personal_rules(personal_rules)
        .with_ares_setting(ares_setting)
        .with_kratos_setting(kratos_setting);
    // Startup reads this game's INIs as its launch does
    // (`rules::ares_switch::ares_way`); a caller that plays the game says so
    // after (`AssetManager::open_for_play`).
    let (startup_rules, projection) = {
        let _ares_way = crate::rules::ares_switch::read_ares_way(assets.ships_ares());
        process.initialize_sources_if_needed(&assets)
    }
    .map_err(anyhow::Error::msg)
    .context("could not load the retail rules")?;
    // Startup registers these before any match computes its agreement.
    let _ = assets.register_neutral_archives();
    process.return_from_loading(assets);
    Ok((process, startup_rules, projection))
}

/// Load the process sources as startup does ([`load_process_sources`]) and
/// report them; `map` is a path to the launcher's map file, and `active_mod`
/// the mod the launch would read (`mods::launch_mod`).
pub(crate) fn probe(
    config: &crate::util::config::GameConfig,
    personal_rules: crate::rules::personal_rules::PersonalRules,
    media_archive_mode: MediaArchiveMode,
    map: Option<&str>,
    active_mod: Option<crate::app::mods::ActiveMod>,
    ares_setting: crate::util::ares_launch::AresSetting,
    kratos_setting: crate::util::kratos_launch::KratosSetting,
) -> Result<ProbeReport> {
    let root = &config.paths.ra2_dir;
    let (mut process, mut startup_rules, projection) = load_process_sources(
        config,
        personal_rules,
        media_archive_mode,
        active_mod,
        ares_setting,
        kratos_setting,
        true,
    )?;
    let assets = process.manager().context("retail sources missing")?;
    assets.open_for_play();
    let modes = crate::skirmish_modes::skirmish_modes_from_assets(assets)?;
    let default_mode = crate::skirmish_modes::default_skirmish_mode(&modes)
        .context("the selected mode catalog has no Skirmish game mode")?;
    // The default mode first, then each mode; the parts read each source
    // once for all of them.
    let files: Vec<&str> = std::iter::once(default_mode)
        .chain(&modes)
        .map(|mode| mode.override_file.as_str())
        .collect();
    let digests = files
        .iter()
        .map(|file| process.multiplayer_rules_digest(file))
        .collect::<Result<Vec<_>, _>>()
        .and_then(|digests| Ok((digests, process.multiplayer_rules_parts(&files)?)))
        .map_err(anyhow::Error::msg)
        .context("could not digest the multiplayer rules")?;
    let mut digests = digests.0.into_iter().zip(digests.1);
    let (rules_digest, rules_parts) = digests.next().context("no default mode")?;
    let game_modes = modes
        .iter()
        .zip(digests)
        .map(|(mode, (rules_digest, rules_parts))| ProbeGameMode {
            id: mode.id,
            rules_digest,
            rules_parts,
        })
        .collect();
    // The Session Country array and PrepareSession's Country bodies, as the
    // shell reads them before a multiplayer start; the digested sources do
    // not change.
    process.read_multiplayer_country(&crate::rules::ini_parser::IniFile::empty(), -1);
    process.refresh_multiplayer_country_bodies();
    // The shell projects those bodies into its Rules before it builds the
    // country menu (`prepare_multiplayer_settings`); the probe does the same.
    if let Some(rules) = startup_rules.as_mut() {
        process.project_multiplayer_countries(rules);
    }
    let csf =
        crate::app::loading::init::load_csf(process.manager().context("retail sources missing")?)?;
    let countries = country_catalog(
        process.multiplayer_countries().map(|(id, _)| id),
        startup_rules.as_ref(),
        &csf,
        process.reads_ares_way(),
    );
    let schemes = probe_color_schemes(startup_rules.as_ref(), projection.as_ref());
    let color_mode = startup_rules
        .as_ref()
        .map(crate::rules::ruleset::RuleSet::skirmish_colors)
        .unwrap_or_default();
    let colors = color_names(&schemes, color_mode);
    let map = map
        .map(|path| {
            spawn_map_identity(
                root,
                path,
                process.manager().context("retail sources missing")?,
            )
        })
        .transpose()?;
    process.refresh_campaigns_if_available();
    // MapSelect keys its stages by the player's side; a campaign's side is
    // the one whose stages hold its first mission.
    let sides: Vec<String> = startup_rules
        .as_ref()
        .map(|rules| {
            (0..=u8::MAX)
                .map_while(|index| rules.side_name(crate::rules::ruleset::SideIdx(index)))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let campaigns = campaign_catalog(root, &process, &sides, &csf)?;
    Ok(ProbeReport {
        version: agreement::version(),
        build_identity: build_identity(),
        rules_digest,
        rules_parts,
        game_modes,
        max_start_waypoints: map.as_ref().map(|map| map.start_waypoints),
        map: map.map(|map| map.map),
        countries,
        colors,
        campaigns,
        generate_map: true,
        max_offline_houses: crate::skirmish_launch::SKIRMISH_MAX_SLOT_COUNT as u8,
        shared_starts: true,
    })
}

/// Each registered Country up to the largest index `Side=` admits, with the
/// label, side and menu eligibility the skirmish shell's catalog gives it.
/// `countries` is the process registry's order, which is the index `Side=`
/// addresses; `rules` holds that registry's projected Country bodies, and
/// `ares` is whether the game is read the Ares way.
fn country_catalog<'a>(
    countries: impl Iterator<Item = &'a str>,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    csf: &CsfFile,
    ares: bool,
) -> Vec<ProbeCountry> {
    use crate::ui::skirmish_shell::{SkirmishComboItem, SkirmishCountryChoice};
    let offered: Vec<u16> = rules
        .map(crate::ui::skirmish_shell::country_choices)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| match item {
            SkirmishComboItem::Country(SkirmishCountryChoice::Country(country)) => {
                Some(country.raw_index())
            }
            _ => None,
        })
        .collect();
    let max_country = max_spawn_country_index(ares);
    countries
        .enumerate()
        .take_while(|(index, _)| *index <= max_country as usize)
        .map(|(index, id)| ProbeCountry {
            index: index as u32,
            name: id.to_owned(),
            label: rules
                .and_then(|rules| rules.country_display_name_sources(id).0)
                .and_then(|key| csf.get(key))
                .unwrap_or_default()
                .to_owned(),
            side: rules
                .and_then(|rules| rules.country_side_index(id))
                .map(|side| side.0),
            selectable: offered.contains(&(index as u16)),
        })
        .collect()
}

/// The process campaign registry with each entry's presentation fields: the
/// same selection of the BATTLEMD/MISSIONMD/MAPSELMD file the INI roots name,
/// and `ra2md.csf`, that the campaign startup and loading screen use.
fn campaign_catalog(
    root: &Path,
    process: &ProcessAssets,
    sides: &[String],
    csf: &CsfFile,
) -> Result<Vec<ProbeCampaign>> {
    let Some(registry) = process.campaigns() else {
        return Ok(Vec::new());
    };
    let assets = process.manager().context("retail sources missing")?;
    let select = |name| crate::rules::retail_sources::select_ini(assets, name).ok();
    let roots = assets.ini_roots();
    let battles = select(roots.battle());
    let missions = select(roots.mission());
    let mapsel = select(roots.mapsel());
    Ok(registry
        .iter()
        .map(|campaign| {
            let section = battles.as_ref().and_then(|b| b.ini.section(campaign.id()));
            let description = section
                .map(|s| s.read_string("Description", "", 128))
                .unwrap_or_default();
            let mission_name = |scenario: &str| {
                missions.as_ref().map_or_else(String::new, |missions| {
                    let mut names =
                        crate::rules::campaign_loading::CampaignScenarioNames::default();
                    names.apply_ini(&missions.ini, scenario, csf);
                    String::from_utf16_lossy(names.name_units())
                })
            };
            let chain = mission_chain(
                root,
                assets,
                mapsel.as_ref().map(|m| &m.ini),
                sides,
                campaign.scenario(),
            )
            .into_iter()
            .map(|scenario| ProbeMission {
                name: mission_name(&scenario),
                scenario,
            })
            .collect();
            ProbeCampaign {
                id: campaign.id().to_owned(),
                scenario: campaign.scenario().to_owned(),
                name: mission_name(campaign.scenario()),
                missions: chain,
                description: csf
                    .get(&description)
                    .map_or(description.clone(), str::to_owned),
                debug_only: section.is_some_and(|s| s.read_bool("DebugOnly", false)),
            }
        })
        .collect())
}

/// Victory's continuation from `first` (`take_campaign_victory`): stop after a
/// `OneTimeOnly` or `EndOfGame` mission, else follow MapSelect's pick. The
/// probe has no running match, so `Global0`'s `AltNextScenario` is not taken.
fn mission_chain(
    root: &Path,
    assets: &AssetManager,
    mapsel: Option<&crate::rules::ini_parser::IniFile>,
    sides: &[String],
    first: &str,
) -> Vec<String> {
    use crate::app::campaign_victory::{MapSelectPick, map_select, map_select_stage_of};
    let side = mapsel.and_then(|mapsel| {
        sides
            .iter()
            .find(|side| map_select_stage_of(mapsel, side, first).is_some())
    });
    let mut stage = side
        .and_then(|side| mapsel.and_then(|mapsel| map_select_stage_of(mapsel, side, first)))
        .unwrap_or(0);
    let mut chain = Vec::new();
    let mut current = first.to_ascii_uppercase();
    // A name campaign startup refuses (`match_bootstrap::prepare_campaign_scenario`)
    // is no mission, and is never handed to the file system here either.
    while chain.len() < MAX_CAMPAIGN_MISSIONS
        && !chain.contains(&current)
        && crate::map::source::is_logical_name(&current)
    {
        chain.push(current.clone());
        let Ok(map) =
            crate::map::source::load_map_ini_by_name_or_path_with_assets(root, &current, assets)
        else {
            break;
        };
        let basic = crate::map::basic::parse_basic_section(&map.ini);
        if basic.one_time_only.unwrap_or(false) || basic.end_of_game.unwrap_or(false) {
            break;
        }
        let skip = basic.skip_map_select.unwrap_or(false);
        let next = basic.next_scenario.unwrap_or_default();
        match map_select(
            mapsel,
            side.map(String::as_str),
            &current,
            skip.then_some((stage, next.as_str())),
        ) {
            MapSelectPick::Next {
                scenario,
                stage: picked,
            } if !scenario.is_empty() => {
                stage = picked;
                current = scenario.to_ascii_uppercase();
            }
            _ => break,
        }
    }
    chain
}

/// The `[Colors]` entries the colours are named from: the startup rules'
/// list, which holds the built-in extra lobby colours the section leaves out
/// (`SkirmishColorMode::from_ini`), or the projection's own section without
/// rules.
fn probe_color_schemes(
    rules: Option<&crate::rules::ruleset::RuleSet>,
    projection: Option<&crate::rules::ini_parser::IniFile>,
) -> Vec<crate::rules::color_scheme::ColorSchemeEntry> {
    match rules {
        Some(rules) => rules.color_schemes.clone(),
        None => projection
            .map(crate::rules::color_scheme::parse_color_schemes)
            .unwrap_or_default(),
    }
}

/// Each `Color=` priority this build admits and the `[Colors]` entry it
/// selects (`Game_GetLinkedColor`). Vanilla offers the lobby's priorities:
/// YR's eight, then VERA's extra colours the rules list; Phobos
/// `SkirmishUnlimitedColors` addresses every entry.
fn color_names(
    schemes: &[crate::rules::color_scheme::ColorSchemeEntry],
    mode: crate::rules::skirmish_colors::SkirmishColorMode,
) -> Vec<IndexedName> {
    let priorities: Vec<i32> = if mode.unlimited() {
        (0..=(schemes.len() as i32 - 1).min(max_spawn_color_index())).collect()
    } else {
        mode.lobby_priorities().map(i32::from).collect()
    };
    priorities
        .into_iter()
        .filter_map(|priority| {
            let entry = mode.entry_for_priority(priority);
            schemes.get(entry).map(|scheme| IndexedName {
                index: priority as u32,
                name: scheme.name.clone(),
            })
        })
        .collect()
}

/// Desktop `--probe [--ra2-dir DIR] [--map PATH]`: the game folder comes from
/// config.toml as for play unless `--ra2-dir` names one, the mod from
/// config.toml unless `--mod` or `--no-mod` chooses, and the Ares setting from
/// VERA.INI unless `--ares` names the one spawn.ini will.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn run_desktop(
    ra2_dir: Option<&Path>,
    map: Option<&Path>,
    mod_switch: Option<&crate::app::mods::ModSwitch>,
    ares: Option<crate::util::ares_launch::AresSetting>,
) -> Result<ProbeReport> {
    let config = crate::util::config::GameConfig::load()?;
    let mut config = match ra2_dir {
        Some(directory) => config.with_retail_directory(
            directory
                .canonicalize()
                .with_context(|| format!("retail directory {}", directory.display()))?,
        )?,
        None => config,
    };
    // RA2 mode as startup takes it: `-ra2` or VERA.INI's choice.
    let profile = existing_profile(config.paths.settings_dir());
    config.use_ra2_mode(profile.ra2_mode);
    let personal_rules = existing_personal_rules(&profile);
    let ares = ares.unwrap_or(profile.ares);
    let map = map
        .map(|path| {
            ensure_file(path)?;
            std::path::absolute(path)?
                .to_str()
                .map(str::to_owned)
                .context("map path is not UTF-8")
        })
        .transpose()?;
    probe(
        &config,
        personal_rules,
        crate::app::frontend::startup_options::RetailStartupOptions::default().media_archive_mode,
        map.as_deref(),
        crate::app::mods::launch_mod(&config.paths, mod_switch).map_err(anyhow::Error::msg)?,
        ares,
        profile.kratos,
    )
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn ensure_file(path: &Path) -> Result<()> {
    anyhow::ensure!(
        crate::util::file_system::is_file(path),
        "map not found: {}",
        path.display()
    );
    Ok(())
}

/// The options the normal startup and the launcher's probes play.
pub(crate) fn existing_profile(
    settings_dir: &Path,
) -> crate::app::persistence::options_profile::RetailOptionsProfile {
    crate::app::persistence::options_profile::RetailOptionsLoad::from_settings(
        settings_dir,
        &Default::default(),
    )
    .retained_profile
}

/// Options shared by a launcher's network match.
pub(crate) fn existing_personal_rules(
    profile: &crate::app::persistence::options_profile::RetailOptionsProfile,
) -> crate::rules::personal_rules::PersonalRules {
    let mut rules = crate::rules::personal_rules::PersonalRules::default();
    rules.set_terrain_deformation(profile.terrain_deformation);
    rules.set_superweapon_limit(i32::from(profile.superweapon_limit));
    // That match plays the player's own Wasm budget too (`cncnet`, which
    // shares the player's own rules), so its digest counts it.
    rules.set_wasm_budget(profile.wasm_budget);
    // Building Production Queue stays off: the probe reports the digest a
    // launcher's network match agrees on, which plays without it
    // (`App::start_spawn_session`).
    rules
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use crate::rules::color_scheme::ColorSchemeEntry;
    use crate::rules::skirmish_colors::SkirmishColorMode;

    fn schemes(count: usize) -> Vec<ColorSchemeEntry> {
        (0..count)
            .map(|index| ColorSchemeEntry {
                name: format!("C{index}"),
                hsv: [0; 3],
            })
            .collect()
    }

    fn agreed_ini_of(directory: &Path) -> crate::app::rules_parts::AgreedIni {
        let profile = existing_profile(directory);
        crate::app::rules_parts::agreed_ini(
            profile.ares,
            profile.kratos,
            profile.ra2_mode,
            &existing_personal_rules(&profile),
        )
    }

    /// The rules parts' `ini` values are what a launcher writes into its
    /// player's VERA.INI to play a host's game: written back, they read as
    /// the same settings, and the probe's rules play them.
    #[test]
    fn agreed_ini_values_read_back_as_the_settings_they_name() {
        use crate::map::source::test_support::TestDirectory;

        let host = TestDirectory::new("agreed-ini-host");
        let guest = TestDirectory::new("agreed-ini-guest");
        host.write(
            "VERA.INI",
            b"[VERA]\nAres=yes\nKratos=on\nTerrainDeformation=true\nSuperweaponLimit=7\nWasmBudget=5000\n",
        );
        let ini = agreed_ini_of(host.path());
        assert_eq!(
            serde_json::to_string(&ini).unwrap(),
            r#"{"VERA":{"Ares":"On","Kratos":"On","RA2Mode":"no","SuperweaponLimit":"7","TerrainDeformation":"yes","WasmBudget":"5000"}}"#
        );
        let written: String = ini["VERA"]
            .iter()
            .map(|(key, value)| format!("{key}={value}\n"))
            .collect();
        guest.write(
            "VERA.INI",
            format!("[VERA]\nFullScreen=yes\n{written}").as_bytes(),
        );
        assert_eq!(agreed_ini_of(guest.path()), ini);
        assert_eq!(
            existing_personal_rules(&existing_profile(guest.path())).wasm_budget(),
            5000
        );
    }

    #[test]
    fn a_campaign_name_that_is_no_map_name_is_no_mission() {
        let root = crate::map::source::test_support::TestDirectory::new("probe-mission-name");
        let assets = AssetManager::from_loose_root_for_test(root.path());
        for name in [r"\\host\share\A.MAP", "../A.MAP"] {
            let chain = mission_chain(root.path(), &assets, None, &[], name);
            assert!(chain.is_empty(), "{name}");
        }
    }

    #[test]
    fn vanilla_colors_are_the_eight_priorities_through_the_linked_color_table() {
        let colors = color_names(&schemes(20), SkirmishColorMode::VANILLA);
        let names: Vec<_> = colors.iter().map(|c| (c.index, c.name.as_str())).collect();
        // PRIORITY_TO_SCHEME_INDEX [3,11,21,29,13,25,17,15] halved.
        assert_eq!(
            names,
            [
                (0, "C1"),
                (1, "C5"),
                (2, "C10"),
                (3, "C14"),
                (4, "C6"),
                (5, "C12"),
                (6, "C8"),
                (7, "C7")
            ]
        );
    }

    #[test]
    fn vanilla_colors_go_on_with_the_extra_lobby_colours_past_the_observers() {
        let colors = color_names(
            &schemes(40),
            SkirmishColorMode::with_extra_entries(&[30, 31]),
        );
        let extras: Vec<_> = colors
            .iter()
            .skip(8)
            .map(|c| (c.index, c.name.as_str()))
            .collect();
        assert_eq!(extras, [(9, "C30"), (10, "C31")]);
    }

    /// Retail `[Colors]` leaves out the built-in extra colours; the rules'
    /// scheme list holds them, so the probe names all thirty.
    #[test]
    fn retail_probe_names_the_thirty_lobby_colours() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
            return;
        };
        let colors = color_names(
            &probe_color_schemes(Some(&rules), None),
            rules.skirmish_colors(),
        );
        let listed: Vec<u32> = colors.iter().map(|c| c.index).collect();
        assert_eq!(listed, (0..8).chain(9..=30).collect::<Vec<u32>>());
        assert_eq!(colors[8].name, "VeraBurgundy");
    }

    /// The desktop probe on retail data offers the ten stock countries on the
    /// sides retail `[Sides]` gives them, each with its menu label.
    #[test]
    fn retail_probe_offers_the_stock_countries_on_their_sides() {
        let Some((root, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let report = run_desktop(Some(&root), None, None, None).unwrap();
        // The launcher reads this before it runs `--generate-map`.
        assert!(report.generate_map);
        let stock = &report.countries[..10];
        assert!(
            stock
                .iter()
                .all(|country| country.selectable && !country.label.is_empty())
        );
        assert_eq!(
            stock.iter().map(|country| country.side).collect::<Vec<_>>(),
            [0, 0, 0, 0, 0, 1, 1, 1, 1, 2].map(Some)
        );
        assert_eq!(stock[9].name, "YuriCountry");
    }

    /// A mod that renames a Country, moves it to another side and hides
    /// another: each row says what the skirmish menu shows for it.
    #[test]
    fn country_catalog_reads_label_side_and_menu_eligibility_from_the_rules() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[Countries]\n0=Americans\n1=China\n2=Hidden\n\
                 [Sides]\nGDI=Americans,Hidden\nNod=China\n\
                 [Americans]\nMultiplay=yes\nUIName=Name:Americans\n\
                 [China]\nMultiplay=yes\nUIName=Name:China\n\
                 [Hidden]\nMultiplay=no\nUIName=Name:Hidden\n",
            ))
            .unwrap();
        let csf = crate::assets::csf_file::test_csf(&[("Name:China", "中国")]);
        let ids: Vec<&str> = (0..3)
            .map(|index| {
                rules
                    .country_name(crate::rules::ruleset::CountryIdx(index))
                    .unwrap()
            })
            .collect();
        let rows: Vec<_> = country_catalog(ids.into_iter(), Some(&rules), &csf, false)
            .into_iter()
            .map(|row| (row.index, row.name, row.label, row.side, row.selectable))
            .collect();
        assert_eq!(
            rows,
            [
                (0, "Americans".into(), String::new(), Some(0), true),
                (1, "China".into(), "中国".into(), Some(1), true),
                (2, "Hidden".into(), String::new(), Some(0), false),
            ]
        );
    }

    #[test]
    fn report_serializes_the_documented_shape() {
        let report = ProbeReport {
            version: "v".into(),
            build_identity: build_identity(),
            rules_digest: "r".into(),
            rules_parts: RulesParts::for_test(),
            game_modes: vec![ProbeGameMode {
                id: 1,
                rules_digest: "r".into(),
                rules_parts: RulesParts::for_test(),
            }],
            map: Some(ProbeMap {
                file: "spawnmap.ini".into(),
                byte_length: 3,
                source_sha256: "s".into(),
                effective_digest: "e".into(),
            }),
            countries: vec![ProbeCountry {
                index: 0,
                name: "Americans".into(),
                label: "America".into(),
                side: Some(0),
                selectable: true,
            }],
            colors: Vec::new(),
            max_start_waypoints: Some(4),
            campaigns: vec![ProbeCampaign {
                id: "ALL1".into(),
                scenario: "ALL01UMD.MAP".into(),
                name: "Allied 01".into(),
                description: "Allied campaign".into(),
                debug_only: false,
                missions: vec![ProbeMission {
                    scenario: "ALL01UMD.MAP".into(),
                    name: "Allied 01".into(),
                }],
            }],
            generate_map: true,
            max_offline_houses: 120,
            shared_starts: true,
        };
        let value = serde_json::to_value(&report).unwrap();
        let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "build_identity",
                "campaigns",
                "colors",
                "countries",
                "game_modes",
                "generate_map",
                "map",
                "max_offline_houses",
                "max_start_waypoints",
                "rules_digest",
                "rules_parts",
                "shared_starts",
                "version"
            ]
        );
        assert_eq!(value["rules_parts"]["v"], 1);
        assert_eq!(value["game_modes"][0]["rules_parts"], value["rules_parts"]);
        let map: Vec<_> = value["map"].as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            map,
            ["byte_length", "effective_digest", "file", "source_sha256"]
        );
        assert_eq!(
            value["countries"][0],
            serde_json::json!({"index":0,"name":"Americans","label":"America","side":0,
                "selectable":true})
        );
        assert_eq!(
            value["campaigns"][0],
            serde_json::json!({"id":"ALL1","scenario":"ALL01UMD.MAP","name":"Allied 01",
                "description":"Allied campaign","debug_only":false,
                "missions":[{"scenario":"ALL01UMD.MAP","name":"Allied 01"}]})
        );
        assert_eq!(
            value["build_identity"]["package_version"],
            env!("CARGO_PKG_VERSION")
        );
        assert!(value["version"].is_string());
        assert_eq!(value["generate_map"], true);
        assert_eq!(value["shared_starts"], true);
        let none = ProbeReport {
            map: None,
            max_start_waypoints: None,
            ..report
        };
        let value = serde_json::to_value(&none).unwrap();
        assert!(value["map"].is_null() && value["max_start_waypoints"].is_null());
    }

    /// `version` is `build_identity`'s package version and network build alone.
    #[test]
    fn version_report_is_the_probe_version_and_build_identity() {
        let value = serde_json::to_value(version_report()).unwrap();
        let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["build_identity", "version"]);
        assert_eq!(value["version"], agreement::version());
        assert_eq!(
            value["build_identity"],
            serde_json::to_value(build_identity()).unwrap()
        );
        let mut identity: Vec<_> = value["build_identity"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        identity.sort();
        assert_eq!(
            identity,
            [
                "commit",
                "network_build",
                "npatch",
                "package_version",
                "phobos",
                "target",
                "verax"
            ]
        );
    }

    #[test]
    fn version_is_the_package_version_and_network_build() {
        let identity = build_identity();
        assert_eq!(
            agreement::version(),
            format!("{}:{}", identity.package_version, identity.network_build)
        );
    }

    #[test]
    fn map_identity_is_the_physical_bytes_and_start_waypoints() {
        use crate::map::source::test_support::TestDirectory;
        let dir = TestDirectory::new("probe-map");
        let bytes = b"[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n[Waypoints]\n0=1001\n1=1002\n2=1003\n[IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n";
        dir.write("spawnmap.ini", bytes);
        let assets = AssetManager::from_loose_root_for_test(dir.path());
        let path = dir.path().join("spawnmap.ini");
        let identity = spawn_map_identity(dir.path(), path.to_str().unwrap(), &assets).unwrap();
        assert_eq!(identity.map.file, "spawnmap.ini");
        assert_eq!(identity.map.byte_length, bytes.len());
        assert_eq!(
            identity.map.source_sha256,
            crate::util::sha256::sha256_hex(bytes)
        );
        assert_eq!(identity.map.effective_digest.len(), 64);
        assert_eq!(identity.start_waypoints, 3);
    }

    #[test]
    fn selected_catalog_probe_reports_custom_mode_and_its_actual_rules() {
        let Some((retail, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let root = crate::map::source::test_support::TestDirectory::new("probe-selected-mode");
        let mod_dir = root.path().join("Mods").join("Catalog");
        std::fs::create_dir_all(&mod_dir).unwrap();
        std::fs::write(mod_dir.join("mod.ini"), "[Files]\nModes=custom-modes.ini\n").unwrap();
        std::fs::write(
            mod_dir.join("custom-modes.ini"),
            "[Battle]\n12=GUI:Custom,STT:Custom,selected-mode.ini,standard,true\n",
        )
        .unwrap();
        std::fs::write(mod_dir.join("selected-mode.ini"), "[General]\nGravity=17\n").unwrap();
        root.write(
            "config.toml",
            format!(
                "[paths]\nra2_dir='{}'\nsettings_dir='{}'\nmod='Catalog'\n",
                retail.display(),
                root.path().display()
            )
            .as_bytes(),
        );
        let config =
            crate::util::config::GameConfig::load_from(root.path(), &root.path().join("vera20k"))
                .unwrap();
        let selected = || crate::app::mods::launch_mod(&config.paths, None).unwrap();
        let report = probe(
            &config,
            Default::default(),
            MediaArchiveMode::STOCK_DIGITAL,
            None,
            selected(),
            crate::util::ares_launch::AresSetting::Auto,
            crate::util::kratos_launch::KratosSetting::Auto,
        )
        .unwrap();
        let (process, _, _) = load_process_sources(
            &config,
            Default::default(),
            MediaArchiveMode::STOCK_DIGITAL,
            selected(),
            crate::util::ares_launch::AresSetting::Auto,
            crate::util::kratos_launch::KratosSetting::Auto,
            false,
        )
        .unwrap();
        let expected = process
            .multiplayer_rules_digest("selected-mode.ini")
            .unwrap();
        assert_eq!(
            report
                .game_modes
                .iter()
                .map(|mode| mode.id)
                .collect::<Vec<_>>(),
            [12]
        );
        assert_eq!(report.game_modes[0].rules_digest, expected);
        assert_eq!(report.rules_digest, expected);
        // The parts name the mode catalog the digest compares, and the mod
        // by its folder in `Mods`.
        let parts = serde_json::to_value(&report.rules_parts).unwrap();
        assert_eq!(
            parts["settings"]["resources"]["roots"][3],
            "custom-modes.ini"
        );
        assert_eq!(parts["settings"]["mod"]["title"], "Catalog");
        assert_ne!(
            report.rules_digest,
            process.multiplayer_rules_digest("MPBattleMD.ini").unwrap()
        );
    }
}
