//! Offline adapter for the CnCNet client/spawner launch contract.
//!
//! Input names/defaults come from CnCNet `yrpp-spawner`
//! `src/Spawner/Spawner.Config.{h,cpp}` (main), not VERA's lobby defaults:
//! https://github.com/CnCNet/yrpp-spawner/tree/main/src/Spawner
//! Slot ordering and campaign difficulty come from `xna-cncnet-client` (develop),
//! `GameLobbyBase.WriteSpawnIni` and `CampaignSelector.LaunchMission`:
//! https://github.com/CnCNet/xna-cncnet-client/tree/develop/DXMainClient/DXGUI
//!
//! This boundary admits one local human, Battle and stock YR campaigns. Ares
//! builds also carry allocated custom Country identities into Battle startup
//! and resolve campaign missions through the retained campaign catalog.
//! It does not load native saves, run network sessions or load extension DLLs.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};

use crate::rules::ini_parser::{IniFile, IniSection};
use crate::skirmish_launch::{
    AiDifficulty, LaunchCountry, LaunchHumanRole, LaunchStartPosition, LaunchTeam,
    PreFillHouseRoster, SkirmishAiSlot, SkirmishHumanSlot, SkirmishLaunchMode,
    SkirmishLaunchOptions, SkirmishLaunchSession, SkirmishLocalSlot,
};

#[derive(Debug)]
pub(crate) struct SpawnRequest {
    pub(crate) game: SpawnGame,
    pub(crate) game_speed: i32,
    pub(crate) seed: Option<u32>,
}

#[derive(Debug)]
pub(crate) enum SpawnGame {
    Skirmish(Box<SkirmishLaunchSession>),
    Campaign(CampaignSpawn),
}

#[derive(Debug)]
pub(crate) struct CampaignSpawn {
    pub(crate) scenario: String,
    pub(crate) scenario_directory: Option<PathBuf>,
    /// Client catalog metadata; this is not an index into retail BATTLEMD.INI.
    pub(crate) campaign_id: i32,
    pub(crate) side: u8,
    /// Campaign human convention: 0 Easy, 1 Normal, 2 Hard.
    pub(crate) difficulty: i32,
    pub(crate) build_off_ally: bool,
}

impl CampaignSpawn {
    /// Normalize an external campaign selection through the ordinary campaign
    /// startup owner. CampaignID remains client metadata; Side is an adapter
    /// selector, never an index into Country/HouseType or the Campaign array.
    pub(crate) fn prepare_startup(
        self,
        catalog: &crate::rules::campaigns::CampaignRegistry,
        game_speed: i32,
        clock: &mut impl crate::app::match_bootstrap::MatchSeedClock,
    ) -> Result<crate::app::match_bootstrap::PreparedCampaignStartup> {
        let stock_index = match self.side {
            0 => catalog.find_index("ALL1"),
            1 => catalog.find_index("SOV1"),
            _ => None,
        };
        #[cfg(feature = "ares")]
        let campaign_index = catalog.find_scenario_index(&self.scenario).or(stock_index);
        #[cfg(not(feature = "ares"))]
        let campaign_index = stock_index;
        let campaign_index = campaign_index.with_context(|| {
            format!(
                "campaign Scenario {:?} has no matching catalog record (Side={})",
                self.scenario, self.side
            )
        })?;
        crate::app::match_bootstrap::prepare_campaign_startup_at_index(
            catalog,
            campaign_index,
            self.difficulty,
            Some(&self.scenario),
            Some(crate::app::match_bootstrap::CampaignLaunchOptions {
                game_speed,
                build_off_ally: self.build_off_ally,
                scenario_directory: self.scenario_directory,
            }),
            clock,
        )
        .with_context(|| {
            format!(
                "campaign record {campaign_index} unavailable for Scenario {:?}",
                self.scenario
            )
        })
    }
}

impl SpawnRequest {
    pub(crate) fn read(path: &Path) -> Result<Self> {
        // The client writes UTF-8 and the Spawner explicitly decodes player
        // names as UTF-8. Retail IniFile::from_bytes instead widens bytes.
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read CnCNet launch file {}", path.display()))?;
        let mut request = Self::parse(&text)
            .with_context(|| format!("invalid CnCNet launch file {}", path.display()))?;
        let absolute = std::path::absolute(path)?;
        let directory = absolute.parent().context("launch file has no directory")?;
        match &mut request.game {
            SpawnGame::Skirmish(session) => {
                // The client writes the final map beside spawn.ini, not in the
                // separate retail asset root. Require that exact file: falling
                // back to an old retail spawnmap.ini could start the wrong game.
                let scenario = directory.join(
                    session
                        .selected_map_file
                        .as_deref()
                        .expect("parsed scenario"),
                );
                ensure!(
                    scenario.is_file(),
                    "client map not found: {}",
                    scenario.display()
                );
                session.selected_map_file = Some(
                    scenario
                        .to_str()
                        .context("client map path is not UTF-8")?
                        .to_owned(),
                );
            }
            SpawnGame::Campaign(campaign) => {
                // Keep the logical mission identity for MISSIONMD/MAPSELMD.
                // Campaign startup resolves loose files from this directory;
                // ordinary retail mission names still fall back to MIX assets.
                campaign.scenario_directory = Some(directory.to_owned());
            }
        }
        Ok(request)
    }

    pub(crate) fn parse(text: &str) -> Result<Self> {
        let ini = IniFile::from_str(text.trim_start_matches('\u{feff}'));
        let settings = ini
            .section("Settings")
            .context("missing [Settings] section")?;
        ensure!(
            !boolean(settings, "Ra2Mode", false)?,
            "Ra2Mode is unsupported; select a Yuri's Revenge mission"
        );
        ensure!(
            !boolean(settings, "ForceMultiplayer", false)?,
            "multiplayer launches are unsupported"
        );
        ensure!(
            integer(settings, "PlayerCount", 1)? == 1,
            "exactly one local human is required"
        );
        ensure!(
            !boolean(settings, "IsSpectator", false)?,
            "spectator launches are unsupported"
        );
        ensure!(
            !boolean(settings, "LoadSaveGame", false)?,
            "CnCNet/native saved games are unsupported"
        );
        ensure!(
            !boolean(settings, "ReadMissionSection", false)?,
            "custom [GameMissionConfig] metadata is unsupported"
        );
        for index in 1..=7 {
            ensure!(
                ini.section(&format!("Other{index}")).is_none(),
                "[Other{index}] requests an additional human player"
            );
        }

        let game_speed = ranged(
            settings,
            "GameSpeed",
            0,
            0,
            i32::from(crate::sim::game_options::IN_GAME_OPTIONS_MAX_SPEED),
        )?;
        let seed = settings
            .is_present("Seed")
            .then(|| integer(settings, "Seed", 0))
            .transpose()?
            .map(|seed| seed as u32);
        let scenario = settings.read_string("Scenario", "spawnmap.ini", 260);
        ensure!(
            !scenario.is_empty(),
            "[Settings] Scenario must not be empty"
        );
        let game = if boolean(settings, "IsSinglePlayer", false)? {
            let difficulty = ranged(settings, "DifficultyModeHuman", 1, 0, 2)?;
            let computer = ranged(settings, "DifficultyModeComputer", 1, 0, 2)?;
            ensure!(
                computer == 2 - difficulty,
                "independent campaign human/computer difficulties are unsupported (expected computer {})",
                2 - difficulty
            );
            SpawnGame::Campaign(CampaignSpawn {
                scenario,
                scenario_directory: None,
                campaign_id: integer(settings, "CampaignID", -1)?,
                side: ranged(
                    settings,
                    "Side",
                    0,
                    0,
                    if crate::rules::ARES_ENABLED {
                        i32::from(u8::MAX)
                    } else {
                        1
                    },
                )? as u8,
                difficulty,
                build_off_ally: boolean(settings, "BuildOffAlly", false)?,
            })
        } else {
            SpawnGame::Skirmish(Box::new(skirmish(&ini, settings, scenario, game_speed)?))
        };
        Ok(Self {
            game,
            game_speed,
            seed,
        })
    }
}

fn skirmish(
    ini: &IniFile,
    settings: &IniSection,
    scenario: String,
    game_speed: i32,
) -> Result<SkirmishLaunchSession> {
    ensure!(
        integer(settings, "GameMode", 1)? == 1,
        "only stock YR Battle (GameMode=1) is supported"
    );
    // The supplied client profile writes No. SpecialHouseIsAlly is a Spawner
    // extension, not part of the stock offline Session's alliance owner.
    ensure!(
        !boolean(settings, "SpecialHouseIsAlly", true)?,
        "SpecialHouseIsAlly=Yes is unsupported; use the VERA client profile"
    );
    let ai_count = ranged(settings, "AIPlayers", 0, 0, 7)? as usize;
    let mut active = vec![0usize];
    let mut opponents = Vec::new();
    let mut used_colors = [false; 8];
    let mut used_starts = [false; 8];
    let local_country = country(settings, "Side")?;
    let local_color = color(settings, "Color", &mut used_colors)?;
    let local_start = start(ini, 0, &mut used_starts)?;
    validate_house_extensions(ini, 0)?;

    for raw_slot in 1..8 {
        let key = format!("Multi{}", raw_slot + 1);
        let handicaps = ini.section_or_empty("HouseHandicaps");
        let difficulty = integer(handicaps, &key, -1)?;
        ensure!(
            (-1..=2).contains(&difficulty),
            "[HouseHandicaps] {key} must be -1 or 0..2"
        );
        if difficulty < 0 {
            ensure!(
                !ini.section_or_empty("HouseCountries").is_present(&key)
                    && !ini.section_or_empty("HouseColors").is_present(&key),
                "{key} has AI options but no active HouseHandicaps entry"
            );
            continue;
        }
        validate_house_extensions(ini, raw_slot)?;
        active.push(raw_slot);
        opponents.push(SkirmishAiSlot {
            raw_slot_index: raw_slot as u8,
            country: country(ini.section_or_empty("HouseCountries"), &key)?,
            country_random: false,
            color_index: color(ini.section_or_empty("HouseColors"), &key, &mut used_colors)?,
            color_random: false,
            start_position: start(ini, raw_slot, &mut used_starts)?,
            team: LaunchTeam::None,
            difficulty: match difficulty {
                0 => AiDifficulty::Hard,
                1 => AiDifficulty::Normal,
                2 => AiDifficulty::Easy,
                _ => unreachable!("active AI difficulty was validated"),
            },
        });
    }
    ensure!(
        opponents.len() == ai_count,
        "AIPlayers={ai_count} does not match {} active HouseHandicaps entries",
        opponents.len()
    );
    // Multi1 is the local human, never an additional AI slot.
    ensure!(
        integer(ini.section_or_empty("HouseHandicaps"), "Multi1", -1)? == -1,
        "[HouseHandicaps] Multi1 conflicts with the local human"
    );
    let teams = teams(ini, &active)?;
    for (opponent, team) in opponents.iter_mut().zip(teams.iter().skip(1)) {
        opponent.team = *team;
    }

    let options = SkirmishLaunchOptions {
        starting_credits: ranged(settings, "Credits", 10000, 0, i32::MAX)?,
        unit_count: ranged(settings, "UnitCount", 0, 0, i32::MAX)?,
        tech_level: ranged(settings, "TechLevel", 10, 0, 10)?,
        game_speed,
        default_ai_difficulty: ranged(settings, "AIDifficulty", 1, 0, 2)?,
        short_game: boolean(settings, "ShortGame", false)?,
        bases: boolean(settings, "Bases", true)?,
        bridges_destroyable: boolean(settings, "BridgeDestroy", true)?,
        super_weapons: boolean(settings, "Superweapons", true)?,
        build_off_ally: boolean(settings, "BuildOffAlly", false)?,
        crates: boolean(settings, "Crates", false)?,
        mcv_redeploy: boolean(settings, "MCVRedeploy", true)?,
        fog_of_war: boolean(settings, "FogOfWar", false)?,
        // Spawner does not write these stock rules-owned switches. Battle's
        // retail defaults remain shroud/growth enabled.
        shroud: true,
        tiberium_grows: true,
        multi_engineer: boolean(settings, "MultiEngineer", false)?,
        harvester_truce: boolean(settings, "HarvesterTruce", false)?,
        ally_change_allowed: boolean(settings, "AlliesAllowed", false)?,
    };
    let mut session = SkirmishLaunchSession {
        // Exact stock descriptor accepted by the shared pre-Fill owner.
        mode: SkirmishLaunchMode {
            class: crate::skirmish_modes::MpModeClass::Battle,
            id: 1,
            ui_name_key: "GUI:Battle".to_string(),
            tooltip_key: "STT:ModeBattle".to_string(),
            override_file: "MPBattleMD.ini".to_string(),
            map_filter: "standard".to_string(),
            random_maps_allowed: true,
            allies_allowed: true,
            must_ally: false,
        },
        selected_map_file: Some(scenario),
        humans: vec![SkirmishHumanSlot {
            raw_slot_index: Some(0),
            name: settings.read_string("Name", "Player", 512),
            slot: SkirmishLocalSlot {
                country: local_country,
                country_random: false,
                color_index: local_color,
                color_random: false,
                start_position: local_start,
                team: teams[0],
            },
            role: LaunchHumanRole::from_observer(false),
        }],
        local_player_index: 0,
        ai_player_count: ai_count as i32,
        opponents,
        // Derived below by the canonical owner from the human and raw AI rows.
        pre_fill_house_roster: PreFillHouseRoster::new(Vec::new(), Vec::new()),
        options,
    };
    session.rebuild_house_roster();
    Ok(session)
}

fn country(section: &IniSection, key: &str) -> Result<LaunchCountry> {
    // The client resolves Random and writes the actual Country array index,
    // not a Side or display-list index. Stock menu4E3A91 caps it at9; Ares's
    // replacement4E3A6A ->1001E2C0 retains Country+B8 without that cap.
    // The shared launch owner validates allocation again after scenario Rules
    // reset; this early reader only admits its canonical identity width.
    let maximum = if crate::rules::ARES_ENABLED {
        i32::from(u16::MAX)
    } else {
        9
    };
    let index = ranged(section, key, -1, 0, maximum)?;
    Ok(LaunchCountry::from_country_index(index as u32))
}

fn color(section: &IniSection, key: &str, used: &mut [bool; 8]) -> Result<u8> {
    // Client [MPColors] GameColorIndex is native color priority, not the
    // [Colors] scheme-entry index. Existing house color binding resolves it.
    let index = ranged(section, key, -1, 0, 7)? as usize;
    ensure!(!used[index], "duplicate player color {index}");
    used[index] = true;
    Ok(index as u8)
}

fn start(ini: &IniFile, raw_slot: usize, used: &mut [bool; 8]) -> Result<LaunchStartPosition> {
    let key = format!("Multi{}", raw_slot + 1);
    let value = ranged(ini.section_or_empty("SpawnLocations"), &key, -2, -2, 7)?;
    if value < 0 {
        return Ok(LaunchStartPosition::Auto);
    }
    ensure!(!used[value as usize], "duplicate starting waypoint {value}");
    used[value as usize] = true;
    Ok(LaunchStartPosition::Position(value as u8))
}

fn validate_house_extensions(ini: &IniFile, raw_slot: usize) -> Result<()> {
    let key = format!("Multi{}", raw_slot + 1);
    ensure!(
        !boolean(ini.section_or_empty("IsSpectator"), &key, false)?,
        "{key} is a spectator"
    );
    ensure!(
        integer(ini.section_or_empty("HandicapDifficulty"), &key, -1)? == -1,
        "per-house HandicapDifficulty is unsupported ({key})"
    );
    ensure!(
        ini.section_or_empty("CreditsFactor").read_double(&key, 1.0) == 1.0,
        "per-house CreditsFactor is unsupported ({key})"
    );
    Ok(())
}

fn teams(ini: &IniFile, active: &[usize]) -> Result<Vec<LaunchTeam>> {
    const KEYS: [&str; 8] = [
        "HouseAllyOne",
        "HouseAllyTwo",
        "HouseAllyThree",
        "HouseAllyFour",
        "HouseAllyFive",
        "HouseAllySix",
        "HouseAllySeven",
        "HouseAllyEight",
    ];
    let mut allies = [[false; 8]; 8];
    for raw_slot in 0..8 {
        let section = ini.section_or_empty(&format!("Multi{}_Alliances", raw_slot + 1));
        allies[raw_slot][raw_slot] = true;
        for key in KEYS {
            let target = ranged(section, key, -1, -1, 7)?;
            if target < 0 {
                continue;
            }
            ensure!(
                active.contains(&raw_slot) && active.contains(&(target as usize)),
                "alliance references an inactive house"
            );
            allies[raw_slot][target as usize] = true;
        }
    }
    // LaunchTeam represents disjoint complete symmetric sets, not arbitrary
    // directed Spawner alliances. Reject lossy conversions at the boundary.
    for &left in active {
        for &right in active {
            ensure!(
                allies[left][right] == allies[right][left],
                "asymmetric alliances are unsupported"
            );
            if allies[left][right] {
                ensure!(
                    active
                        .iter()
                        .all(|&third| allies[left][third] == allies[right][third]),
                    "overlapping non-team alliances are unsupported"
                );
            }
        }
    }
    Ok(active
        .iter()
        .map(|&slot| {
            if active.iter().filter(|&&other| allies[slot][other]).count() > 1 {
                LaunchTeam::Team(
                    active
                        .iter()
                        .copied()
                        .find(|&other| allies[slot][other])
                        .expect("self ally") as u8,
                )
            } else {
                LaunchTeam::None
            }
        })
        .collect())
}

fn boolean(section: &IniSection, key: &str, default: bool) -> Result<bool> {
    if !section.is_present(key) {
        return Ok(default);
    }
    section
        .read_bool_value(key)
        .with_context(|| format!("[{}] {key} is not a boolean", section.name))
}

fn ranged(section: &IniSection, key: &str, default: i32, min: i32, max: i32) -> Result<i32> {
    let value = integer(section, key, default)?;
    ensure!(
        (min..=max).contains(&value),
        "[{}] {key}={value} must be in {min}..={max}",
        section.name
    );
    Ok(value)
}

fn integer(section: &IniSection, key: &str, default: i32) -> Result<i32> {
    // Keep the native reader's signed/hex semantics, while refusing malformed
    // external configuration instead of accepting its permissive atoi zero.
    if section.is_present(key)
        && !section.read_string_with(key, 512, false, |_, text| {
            let (digits, radix, maximum) = if let Some(hex) = text.strip_prefix('$') {
                (hex, 16u32, u32::MAX)
            } else if let Some(hex) = text.strip_suffix(['h', 'H']) {
                (hex, 16u32, u32::MAX)
            } else {
                let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
                let maximum = if text.starts_with('-') {
                    0x8000_0000
                } else {
                    0x7fff_ffff
                };
                (digits, 10u32, maximum)
            };
            // Validation only: the shared native reader below remains the
            // value parser. Do not let overflow wrap an invalid slot to zero.
            !digits.is_empty()
                && digits
                    .bytes()
                    .try_fold(0u32, |total, byte| {
                        let digit = (byte as char).to_digit(radix)?;
                        total
                            .checked_mul(radix)?
                            .checked_add(digit)
                            .filter(|value| *value <= maximum)
                    })
                    .is_some()
        })
    {
        bail!("[{}] {key} is not an integer", section.name);
    }
    Ok(section.read_int(key, default))
}

#[cfg(test)]
#[path = "cncnet_spawn_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cncnet_campaign_spawn_tests.rs"]
mod campaign_tests;
