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
//! builds also carry allocated custom Country identities into Battle startup.
//! External campaigns pass an explicit Scenario independently of record0.
//! Multiplayer Battle files (`PlayerCount>1`, docs/spawn.md) normalize into a
//! canonical source-ordered session plus a relay or direct transport.
//! It does not load native saves or load extension DLLs.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail, ensure};

use crate::rules::ini_parser::{IniFile, IniSection};
use crate::skirmish_launch::{
    AiDifficulty, LaunchCountry, LaunchHumanRole, LaunchStartPosition, LaunchTeam,
    NETWORK_PLAYER_SLOT_COUNT, PreFillHouseRoster, SKIRMISH_PLAYER_SLOT_COUNT, SkirmishAiSlot,
    SkirmishHumanSlot, SkirmishLaunchMode, SkirmishLaunchOptions, SkirmishLaunchSession,
    SkirmishLocalSlot,
};

#[derive(Debug)]
pub(crate) struct SpawnRequest {
    pub(crate) game: SpawnGame,
    pub(crate) game_speed: i32,
    pub(crate) seed: Option<u32>,
    /// *(VERA)* Optional `[Settings] Expected*` values a launcher read from
    /// `vera20k --probe`; the launch is rejected at start when they differ.
    pub(crate) expected: SpawnExpectations,
}

/// The mode file every spawn Battle selects (`GameMode=1`); the multiplayer
/// rules agreement and `--probe` digest the rules through this mode.
pub(crate) const BATTLE_MODE_OVERRIDE_FILE: &str = "MPBattleMD.ini";

/// `[Settings] ExpectedVersion`, `ExpectedRulesDigest`, `ExpectedMapDigest`
/// (docs/spawn.md). Absent values are not checked.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct SpawnExpectations {
    pub(crate) version: Option<String>,
    /// Lowercase SHA-256 hex of the multiplayer rules agreement.
    pub(crate) rules_digest: Option<String>,
    /// Lowercase SHA-256 hex of the client map's physical bytes.
    pub(crate) map_digest: Option<String>,
}

impl SpawnExpectations {
    fn parse(settings: &IniSection) -> Result<Self> {
        let digest = |key: &str| -> Result<Option<String>> {
            if !settings.is_present(key) {
                return Ok(None);
            }
            let value = text(settings, key)?.to_ascii_lowercase();
            ensure!(
                value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "[Settings] {key} must be 64 hexadecimal digits"
            );
            Ok(Some(value))
        };
        Ok(Self {
            version: settings
                .is_present("ExpectedVersion")
                .then(|| text(settings, "ExpectedVersion"))
                .transpose()?,
            rules_digest: digest("ExpectedRulesDigest")?,
            map_digest: digest("ExpectedMapDigest")?,
        })
    }

    /// The first expectation this build, its rules or the map contradicts.
    pub(crate) fn mismatch(
        &self,
        version: &str,
        rules_digest: impl FnOnce() -> Result<String>,
        map_digest: impl FnOnce() -> Result<String>,
    ) -> Result<()> {
        if let Some(expected) = &self.version {
            ensure!(
                expected == version,
                "ExpectedVersion {expected:?} does not match this build's version {version:?}"
            );
        }
        if let Some(expected) = &self.rules_digest {
            let actual = rules_digest()?;
            ensure!(
                *expected == actual,
                "ExpectedRulesDigest {expected} does not match the local rules digest {actual}"
            );
        }
        if let Some(expected) = &self.map_digest {
            let actual = map_digest()?;
            ensure!(
                *expected == actual,
                "ExpectedMapDigest {expected} does not match the client map's SHA-256 {actual}"
            );
        }
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) enum SpawnGame {
    Skirmish(Box<SkirmishLaunchSession>),
    Campaign(CampaignSpawn),
    Network(Box<NetworkSpawn>),
}

/// A multiplayer Battle launch. `session.humans` is in canonical SourceIndex
/// order and every row index is shared, so each participant derives the same
/// session apart from `local_player_index`.
#[derive(Debug)]
pub(crate) struct NetworkSpawn {
    pub(crate) session: SkirmishLaunchSession,
    pub(crate) transport: SpawnTransport,
}

#[derive(Debug)]
pub(crate) enum SpawnTransport {
    Relay(RelaySpawn),
    /// Desktop direct UDP between the listed endpoints (VERA LAN protocol).
    Direct(DirectSpawn),
}

/// The `game_started` identity and grant a launcher service handed over.
pub(crate) struct RelaySpawn {
    pub(crate) address: SocketAddrV4,
    pub(crate) match_id: String,
    pub(crate) player_id: String,
    pub(crate) grant: crate::app::shell_wol::DecodedRelayGrant,
    /// HTTPS origin (loopback HTTP allowed) of the match API.
    pub(crate) service_url: String,
    /// Bearer for `service_url`.
    pub(crate) token: String,
    /// The launch speed the ordering certificate's fixed clock names.
    pub(crate) game_speed: i32,
    /// `[Relay] AdmissionTimeout`: each admission barrier's client deadline.
    pub(crate) admission_timeout: crate::util::time::Duration,
}

// The grant and bearer are credentials; never let a launch log print them.
impl std::fmt::Debug for RelaySpawn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelaySpawn")
            .field("address", &self.address)
            .field("match_id", &self.match_id)
            .field("player_id", &self.player_id)
            .field("service_url", &self.service_url)
            .field("game_speed", &self.game_speed)
            .field("admission_timeout", &self.admission_timeout)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub(crate) struct DirectSpawn {
    pub(crate) local_port: u16,
    /// Indexed by SourceIndex; `None` at the local source.
    pub(crate) peers: Vec<Option<SocketAddrV4>>,
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
    /// startup owner. CampaignID and Side remain client metadata, never indexes
    /// into Country/HouseType or the Campaign array.
    pub(crate) fn prepare_startup(
        self,
        catalog: &crate::rules::campaigns::CampaignRegistry,
        game_speed: i32,
        clock: &mut impl crate::app::match_bootstrap::MatchSeedClock,
    ) -> Result<crate::app::match_bootstrap::PreparedCampaignStartup> {
        // CnCNet/yrpp-spawner de64f070, Spawner.cpp327 passes
        // StartScenario(ScenarioName,1,0). Original683AB0 keeps this index
        // independently of the explicit filename; see campaign_start_prefix.json.
        // Do not infer a record from Scenario, Side or client CampaignID.
        let campaign_index = 0;
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
        let text = crate::util::file_system::read_to_string(path)
            .with_context(|| format!("cannot read CnCNet launch file {}", path.display()))?;
        let mut request = Self::parse(&text)
            .with_context(|| format!("invalid CnCNet launch file {}", path.display()))?;
        // The browser's in-memory paths are rooted already, and
        // `std::path::absolute` is unsupported on wasm32-unknown-unknown
        // (where `is_absolute` also requires a Windows-style prefix).
        let absolute = if path.is_absolute() || cfg!(target_os = "unknown") && path.has_root() {
            path.to_owned()
        } else {
            std::path::absolute(path)?
        };
        let directory = absolute.parent().context("launch file has no directory")?;
        match &mut request.game {
            SpawnGame::Skirmish(session) => resolve_client_map(directory, session)?,
            SpawnGame::Network(network) => resolve_client_map(directory, &mut network.session)?,
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
        let player_count = ranged(
            settings,
            "PlayerCount",
            1,
            1,
            NETWORK_PLAYER_SLOT_COUNT as i32,
        )? as usize;
        let multiplayer = player_count > 1;
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
        for index in player_count..NETWORK_PLAYER_SLOT_COUNT {
            ensure!(
                ini.section(&format!("Other{index}")).is_none(),
                "[Other{index}] requests an additional human player"
            );
        }
        ensure!(
            multiplayer || ini.section("Relay").is_none(),
            "[Relay] requires PlayerCount>1"
        );

        let mut game_speed = ranged(
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
        // Every participant must draw the same match RNG stream.
        ensure!(
            !multiplayer || seed.is_some(),
            "multiplayer launches require [Settings] Seed"
        );
        let expected = SpawnExpectations::parse(settings)?;
        let scenario = settings.read_string("Scenario", "spawnmap.ini", 260);
        ensure!(
            !scenario.is_empty(),
            "[Settings] Scenario must not be empty"
        );
        let game = if boolean(settings, "IsSinglePlayer", false)? {
            ensure!(!multiplayer, "multiplayer campaigns are unsupported");
            ensure!(
                expected.rules_digest.is_none() && expected.map_digest.is_none(),
                "ExpectedRulesDigest and ExpectedMapDigest apply to Battle launches only"
            );
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
        } else if multiplayer {
            let mut session = skirmish(&ini, settings, scenario, game_speed, player_count)?;
            let transport = transport(&ini, settings, &session)?;
            if let SpawnTransport::Relay(relay) = &transport {
                // The relay runs the certificate's fixed clock; a different
                // local speed would desynchronize every participant.
                ensure!(
                    !settings.is_present("GameSpeed") || game_speed == relay.game_speed,
                    "[Settings] GameSpeed={game_speed} disagrees with the relay clock (GameSpeed={})",
                    relay.game_speed
                );
                game_speed = relay.game_speed;
                session.options.game_speed = game_speed;
            }
            SpawnGame::Network(Box::new(NetworkSpawn { transport, session }))
        } else {
            SpawnGame::Skirmish(Box::new(skirmish(
                &ini,
                settings,
                scenario,
                game_speed,
                player_count,
            )?))
        };
        Ok(Self {
            game,
            game_speed,
            seed,
            expected,
        })
    }
}

fn resolve_client_map(directory: &Path, session: &mut SkirmishLaunchSession) -> Result<()> {
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
        crate::util::file_system::is_file(&scenario),
        "client map not found: {}",
        scenario.display()
    );
    session.selected_map_file = Some(
        scenario
            .to_str()
            .context("client map path is not UTF-8")?
            .to_owned(),
    );
    Ok(())
}

/// House slot `MultiN` is index N-1: the local human is house 0, `[OtherK]`
/// house K, and AI houses follow every human. Session rows are shared by all
/// participants instead: a human's row is its SourceIndex and an AI's row is
/// its house slot, so both forms coincide for a single player.
fn skirmish(
    ini: &IniFile,
    settings: &IniSection,
    scenario: String,
    game_speed: i32,
    player_count: usize,
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
    let multiplayer = player_count > 1;
    let house_slots = if multiplayer {
        NETWORK_PLAYER_SLOT_COUNT
    } else {
        SKIRMISH_PLAYER_SLOT_COUNT
    };
    let ai_count = ranged(
        settings,
        "AIPlayers",
        0,
        0,
        (house_slots - player_count) as i32,
    )? as usize;
    let handicaps = ini.section_or_empty("HouseHandicaps");
    let mut active = Vec::new();
    let mut rows = Vec::new();
    let mut colors = Vec::new();
    let mut used_starts = [false; NETWORK_PLAYER_SLOT_COUNT];
    let mut humans: Vec<Option<SkirmishHumanSlot>> = vec![None; player_count];
    let mut names = std::collections::BTreeSet::new();
    let mut local_player_index = 0;

    for house in 0..player_count {
        let section = if house == 0 {
            settings
        } else {
            ini.section(&format!("Other{house}"))
                .with_context(|| format!("PlayerCount={player_count} requires [Other{house}]"))?
        };
        let source = if multiplayer {
            source_index(section, player_count)?
        } else {
            0
        };
        ensure!(humans[source].is_none(), "duplicate SourceIndex {source}");
        let name = if multiplayer {
            let name = section.read_string("Name", "", 512);
            ensure!(!name.is_empty(), "[{}] Name is required", section.name);
            // Network sessions bind peers by name; native names are
            // case-insensitive.
            ensure!(
                names.insert(name.to_lowercase()),
                "duplicate player name {name:?}"
            );
            name
        } else {
            section.read_string("Name", "Player", 512)
        };
        // docs/spawn.md: a multiplayer human may watch. Spectators take no
        // country, colour or start; scenario startup normalizes them.
        let observer = spectator(ini, house)?;
        ensure!(
            multiplayer || !observer,
            "Multi{} is a spectator",
            house + 1
        );
        let (country, color) = if observer {
            (LaunchCountry::from_country_index(0), 0)
        } else {
            (country(section, "Side")?, color(section, "Color")?)
        };
        let start = start(ini, house, house_slots, &mut used_starts)?;
        ensure!(
            !observer || start == LaunchStartPosition::Auto,
            "spectator Multi{} cannot have a start location",
            house + 1
        );
        validate_house_extensions(ini, house)?;
        // MultiN of a human is never an additional AI slot.
        let key = format!("Multi{}", house + 1);
        ensure!(
            integer(handicaps, &key, -1)? == -1,
            "[HouseHandicaps] {key} conflicts with a human player"
        );
        if house == 0 {
            local_player_index = source;
        }
        active.push(house);
        rows.push(source as u8);
        if !observer {
            colors.push(color);
        }
        humans[source] = Some(SkirmishHumanSlot {
            raw_slot_index: Some(source as u8),
            name,
            slot: SkirmishLocalSlot {
                country,
                country_random: false,
                color_index: color,
                color_random: false,
                start_position: start,
                team: LaunchTeam::None,
            },
            role: LaunchHumanRole::from_observer(observer),
        });
    }
    // Each source in 0..PlayerCount was taken exactly once above.
    let mut humans: Vec<_> = humans
        .into_iter()
        .map(|human| human.expect("SourceIndex permutation"))
        .collect();

    let mut opponents = Vec::new();
    for raw_slot in player_count..house_slots {
        let key = format!("Multi{}", raw_slot + 1);
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
        ensure!(!spectator(ini, raw_slot)?, "{key} is a spectator");
        validate_house_extensions(ini, raw_slot)?;
        let color_index = color(ini.section_or_empty("HouseColors"), &key)?;
        active.push(raw_slot);
        rows.push(raw_slot as u8);
        colors.push(color_index);
        opponents.push(SkirmishAiSlot {
            raw_slot_index: raw_slot as u8,
            country: country(ini.section_or_empty("HouseCountries"), &key)?,
            country_random: false,
            color_index,
            color_random: false,
            start_position: start(ini, raw_slot, house_slots, &mut used_starts)?,
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
    // The lobby's start admission (services/lobby/protocol.md): spectators
    // exclude AI, and at least two participants play.
    let spectators = humans.iter().filter(|human| human.is_observer()).count();
    ensure!(
        spectators == 0 || ai_count == 0,
        "games with spectators cannot include AI players"
    );
    ensure!(
        !multiplayer || humans.len() - spectators + ai_count >= 2,
        "a multiplayer launch needs at least two playing participants"
    );
    // Network rooms reuse YR's eight colours once more than eight houses
    // play (see `network_color_in_use`); up to eight they stay distinct.
    if colors.len() <= SKIRMISH_PLAYER_SLOT_COUNT {
        let mut used = [false; COLOR_SLOTS];
        for &index in &colors {
            ensure!(
                !std::mem::replace(&mut used[usize::from(index)], true),
                "duplicate player color {index}"
            );
        }
    }
    // `active` lists the humans' houses first, then the AI houses in order.
    let teams = teams(ini, &active, &rows)?;
    for (row, team) in rows.iter().zip(&teams).take(player_count) {
        let human = &mut humans[usize::from(*row)];
        ensure!(
            !human.is_observer() || *team == LaunchTeam::None,
            "spectator {} cannot belong to a team",
            human.name
        );
        human.slot.team = *team;
    }
    for (opponent, team) in opponents.iter_mut().zip(&teams[player_count..]) {
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
            override_file: BATTLE_MODE_OVERRIDE_FILE.to_string(),
            map_filter: "standard".to_string(),
            random_maps_allowed: true,
            allies_allowed: true,
            must_ally: false,
        },
        selected_map_file: Some(scenario),
        humans,
        local_player_index,
        ai_player_count: ai_count as i32,
        opponents,
        // Derived below by the canonical owner from the human and raw AI rows.
        pre_fill_house_roster: PreFillHouseRoster::new(Vec::new(), Vec::new()),
        options,
    };
    session.rebuild_house_roster();
    Ok(session)
}

/// The largest `Side=` Country index this build reads (`--probe` lists them).
pub(crate) const fn max_spawn_country_index() -> i32 {
    // The client resolves Random and writes the actual Country array index,
    // not a Side or display-list index. Stock menu4E3A91 caps it at9; Ares's
    // replacement4E3A6A ->1001E2C0 retains Country+B8 without that cap.
    // The shared launch owner validates allocation again after scenario Rules
    // reset; this early reader only admits its canonical identity width.
    if crate::rules::ARES_ENABLED {
        u16::MAX as i32
    } else {
        9
    }
}

/// The largest `Color=` value this build reads (see [`color`]).
pub(crate) const fn max_spawn_color_index() -> i32 {
    if crate::rules::PHOBOS_ENABLED { 255 } else { 7 }
}

fn country(section: &IniSection, key: &str) -> Result<LaunchCountry> {
    let index = ranged(section, key, -1, 0, max_spawn_country_index())?;
    Ok(LaunchCountry::from_country_index(index as u32))
}

/// Colour slots this reader admits, one per byte the spawner can send.
const COLOR_SLOTS: usize = 256;

fn color(section: &IniSection, key: &str) -> Result<u8> {
    // Client [MPColors] GameColorIndex is native color priority, not the
    // [Colors] scheme-entry index. Existing house color binding resolves it
    // through Game_GetLinkedColor (0x69A310).
    //
    // A phobos build admits Phobos SkirmishUnlimitedColors, where the spawner
    // sends the [Colors] entry index instead; that build therefore accepts the
    // whole byte and the loaded rules decide which entries exist. Without the
    // feature the stock client only produces YR's eight priorities.
    Ok(ranged(section, key, -1, 0, max_spawn_color_index())? as u8)
}

/// Waypoints 0..house_slots-1: eight for a single player, thirty in network
/// rooms; the map decides which exist.
fn start(
    ini: &IniFile,
    raw_slot: usize,
    house_slots: usize,
    used: &mut [bool; NETWORK_PLAYER_SLOT_COUNT],
) -> Result<LaunchStartPosition> {
    let key = format!("Multi{}", raw_slot + 1);
    let value = ranged(
        ini.section_or_empty("SpawnLocations"),
        &key,
        -2,
        -2,
        house_slots as i32 - 1,
    )?;
    if value < 0 {
        return Ok(LaunchStartPosition::Auto);
    }
    ensure!(!used[value as usize], "duplicate starting waypoint {value}");
    used[value as usize] = true;
    Ok(LaunchStartPosition::Position(value as u8))
}

/// `[IsSpectator] MultiN`; only multiplayer humans may set it.
fn spectator(ini: &IniFile, raw_slot: usize) -> Result<bool> {
    boolean(
        ini.section_or_empty("IsSpectator"),
        &format!("Multi{}", raw_slot + 1),
        false,
    )
}

fn validate_house_extensions(ini: &IniFile, raw_slot: usize) -> Result<()> {
    let key = format!("Multi{}", raw_slot + 1);
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

/// `active` holds house slots (MultiN-1); `rows` the matching shared session
/// rows, which name a team so that every participant derives the same value.
fn teams(ini: &IniFile, active: &[usize], rows: &[u8]) -> Result<Vec<LaunchTeam>> {
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
    let legacy = (0..NETWORK_PLAYER_SLOT_COUNT).any(|slot| {
        ini.section(&format!("Multi{}_Alliances", slot + 1))
            .is_some()
    });
    if let Some(section) = ini.section("HouseTeams") {
        // VERA form: one shared team number per team is complete and
        // symmetric by construction.
        ensure!(
            !legacy,
            "[HouseTeams] cannot be combined with [MultiN_Alliances]"
        );
        for slot in 0..NETWORK_PLAYER_SLOT_COUNT {
            let key = format!("Multi{}", slot + 1);
            ensure!(
                active.contains(&slot) || !section.is_present(&key),
                "[HouseTeams] {key} names an inactive house"
            );
        }
        return active
            .iter()
            .map(|slot| {
                let key = format!("Multi{}", slot + 1);
                Ok(if section.is_present(&key) {
                    let last = NETWORK_PLAYER_SLOT_COUNT as i32 - 1;
                    LaunchTeam::Team(ranged(section, &key, -1, 0, last)? as u8)
                } else {
                    LaunchTeam::None
                })
            })
            .collect();
    }
    // Spawner alliances name houses 1..8 only.
    for slot in SKIRMISH_PLAYER_SLOT_COUNT..NETWORK_PLAYER_SLOT_COUNT {
        ensure!(
            ini.section(&format!("Multi{}_Alliances", slot + 1))
                .is_none(),
            "[Multi{}_Alliances] is beyond the eight legacy houses; use [HouseTeams]",
            slot + 1
        );
    }
    let mut allies = [[false; NETWORK_PLAYER_SLOT_COUNT]; NETWORK_PLAYER_SLOT_COUNT];
    for raw_slot in 0..NETWORK_PLAYER_SLOT_COUNT {
        allies[raw_slot][raw_slot] = true;
    }
    for raw_slot in 0..SKIRMISH_PLAYER_SLOT_COUNT {
        let section = ini.section_or_empty(&format!("Multi{}_Alliances", raw_slot + 1));
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
                        .zip(rows)
                        .filter(|&(&other, _)| allies[slot][other])
                        .map(|(_, &row)| row)
                        .min()
                        .expect("self ally"),
                )
            } else {
                LaunchTeam::None
            }
        })
        .collect())
}

fn source_index(section: &IniSection, player_count: usize) -> Result<usize> {
    ensure!(
        section.is_present("SourceIndex"),
        "[{}] SourceIndex is required in multiplayer",
        section.name
    );
    Ok(ranged(section, "SourceIndex", -1, 0, player_count as i32 - 1)? as usize)
}

fn transport(
    ini: &IniFile,
    settings: &IniSection,
    session: &SkirmishLaunchSession,
) -> Result<SpawnTransport> {
    let player_count = session.humans.len();
    if let Some(relay) = ini.section("Relay") {
        let address = text(relay, "Address")?
            .parse::<SocketAddrV4>()
            .map_err(|_| anyhow!("[Relay] Address must be IPv4:port"))?;
        let match_id = text(relay, "MatchId")?;
        let player_id = text(relay, "PlayerId")?;
        // The lobby's decoder checks the ticket's match, player and source
        // (RelayGrant.source_index is SourceIndex) and the key widths.
        let grant = crate::app::shell_wol::decode_external_relay_grant(
            address,
            text(relay, "Ticket")?,
            text(relay, "PacketKey")?,
            text(relay, "PayloadKey")?,
            text(relay, "OrderingCertificate")?,
            // RelayGrant.federation: present when the service homes the match
            // on several regional relays.
            relay
                .is_present("Federation")
                .then(|| text(relay, "Federation"))
                .transpose()?,
            session.local_player_index as u8,
            &match_id,
            &player_id,
        )
        .context("invalid [Relay] grant")?;
        let certificate = crate::net::udp::OrderingCertificate::parse(&grant.ordering_certificate)
            .map_err(|_| anyhow!("invalid [Relay] OrderingCertificate"))?;
        ensure!(
            certificate.sources_mask == (1u32 << player_count) - 1,
            "[Relay] OrderingCertificate roster does not match PlayerCount={player_count}"
        );
        // The lobby derives this clock from the launch speed table
        // [60,45,30,20,15,12,10], the native PreCalcFrameRate rows.
        let game_speed = crate::net::lockstep::game_speed_for_frame_rate(certificate.tick_hz)
            .with_context(|| {
                format!(
                    "[Relay] OrderingCertificate clock {} Hz is not a launch speed",
                    certificate.tick_hz
                )
            })?;
        let token = text(relay, "Token")?;
        ensure!(
            token.bytes().all(|byte| byte.is_ascii_graphic()),
            "[Relay] Token must be visible ASCII"
        );
        let admission_timeout = ranged(relay, "AdmissionTimeout", 30, 30, 180)?;
        return Ok(SpawnTransport::Relay(RelaySpawn {
            address,
            match_id,
            player_id,
            grant,
            service_url: service_origin(&text(relay, "ServiceUrl")?)?,
            token,
            game_speed,
            admission_timeout: crate::util::time::Duration::from_secs(admission_timeout as u64),
        }));
    }
    let mut peers = vec![None; player_count];
    for house in 1..player_count {
        let section = ini
            .section(&format!("Other{house}"))
            .expect("validated human section");
        let ip = text(section, "Ip")?
            .parse::<Ipv4Addr>()
            .map_err(|_| anyhow!("[{}] Ip must be an IPv4 address", section.name))?;
        ensure!(
            !ip.is_unspecified() && !ip.is_broadcast() && !ip.is_multicast(),
            "[{}] Ip must name one host",
            section.name
        );
        let endpoint = SocketAddrV4::new(ip, port(section)?);
        ensure!(
            !peers.contains(&Some(endpoint)),
            "duplicate direct endpoint {endpoint}"
        );
        peers[source_index(section, player_count)?] = Some(endpoint);
    }
    Ok(SpawnTransport::Direct(DirectSpawn {
        local_port: port(settings)?,
        peers,
    }))
}

fn port(section: &IniSection) -> Result<u16> {
    let default = i32::from(crate::net::udp::DEFAULT_LAN_PORT);
    Ok(ranged(section, "Port", default, 1, i32::from(u16::MAX))? as u16)
}

/// `ServiceUrl` is an origin: HTTPS, or HTTP to a loopback host for
/// development. Returned without a trailing slash.
fn service_origin(url: &str) -> Result<String> {
    let origin = url.strip_suffix('/').unwrap_or(url);
    let (secure, authority) = if let Some(authority) = origin.strip_prefix("https://") {
        (true, authority)
    } else if let Some(authority) = origin.strip_prefix("http://") {
        (false, authority)
    } else {
        bail!("[Relay] ServiceUrl must be an https:// origin");
    };
    ensure!(
        !authority.is_empty()
            && authority.bytes().all(|byte| byte.is_ascii_graphic())
            && !authority.contains(['/', '\\', '@', '?', '#']),
        "[Relay] ServiceUrl must be an origin without path, query or credentials"
    );
    let host = match authority.rfind(':') {
        Some(colon) if !authority[colon..].contains(']') => &authority[..colon],
        _ => authority,
    };
    ensure!(
        secure
            || host.eq_ignore_ascii_case("localhost")
            || host == "[::1]"
            || host.parse::<Ipv4Addr>().is_ok_and(|ip| ip.is_loopback()),
        "[Relay] ServiceUrl must use HTTPS (HTTP only for loopback)"
    );
    Ok(origin.to_owned())
}

/// Addresses and credentials: required, and never silently cut by ReadString.
fn text(section: &IniSection, key: &str) -> Result<String> {
    const LIMIT: usize = 4096;
    let value = section.read_string(key, "", LIMIT + 2);
    ensure!(!value.is_empty(), "[{}] {key} is required", section.name);
    ensure!(value.len() <= LIMIT, "[{}] {key} is too long", section.name);
    Ok(value)
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
