//! App-owned match startup authority and pre-first-tick evidence.
//!
//! This module sits above `sim/`: it classifies explicit-start Battle
//! sessions, reads the ordinary Windows seed once, and observes an already
//! initialized `Simulation` without mutating it.

use crate::rules::rule_policy::{BootstrapRuleProfile, RulePolicy};
use crate::sim::campaign_carry_over::CampaignCarryOver;
use crate::sim::scenario_session::{ExecutionTopology, ScenarioGameMode};
use crate::sim::world::{Simulation, SimulationRngState};
use crate::skirmish_launch::{LaunchStartPosition, SKIRMISH_AI_SLOT_COUNT, SkirmishLaunchSession};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct MatchCorrelationId(u64);

impl MatchCorrelationId {
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchSeedSource {
    WindowsGetTickCount,
    Controlled,
    NonWindowsDevelopmentFallback,
    /// Seed supplied by the admitted host start, never sampled by a guest.
    AgreedNetworkHost,
    /// Retained Scenario seed; reconstructing saved campaign admission does
    /// not sample a fresh seed. Restart/next mission do so at their own entry.
    SavedScenario,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MatchSeed {
    pub value: u32,
    pub source: MatchSeedSource,
    /// This describes only the seed source, never whole-startup or MapGen parity.
    pub seed_authority_certifying: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptedBattleSession {
    launch: SkirmishLaunchSession,
}

impl AcceptedBattleSession {
    pub fn launch_session(&self) -> &SkirmishLaunchSession {
        &self.launch
    }

    pub fn selected_map_file(&self) -> &str {
        self.launch
            .selected_map_file
            .as_deref()
            .expect("accepted sessions always own a selected map record")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnverifiedStartupReason {
    MissingMap,
    NonBattleMode,
    RandomLocalCountry,
    RandomAiCountry { index: usize },
    RandomLocalColor,
    RandomAiColor { index: usize },
    AutoLocalStart,
    AutoAiStart { index: usize },
    InvalidActiveAiCount { count: usize },
    IncompleteLedger,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StartupSessionClassification {
    AcceptedExplicitFixedBattle(Box<AcceptedBattleSession>),
    UnverifiedLegacy(UnverifiedStartupReason),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedMatchStartup {
    pub correlation: MatchCorrelationId,
    pub seed: MatchSeed,
    pub session: AcceptedBattleSession,
}

/// Immutable accepted network handoff, shared with the live session owner.
/// No UI or loading phase keeps a second mutable roster after this freeze.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedNetworkStartup {
    launch: SkirmishLaunchSession,
    authored_campaign: Option<PreparedCampaignStart>,
    /// Physical local bytes, independent of the logical map name folded into
    /// shared scenario state. Cache paths differ legitimately across machines.
    local_map_source: Option<crate::map::source::LoadedMapSource>,
    local_map_effective_digest: Option<String>,
    local_random_map_preview: Option<crate::map::source::LoadedMapSource>,
    pub(crate) seed: MatchSeed,
    mode: crate::app::match_runtime::session::SessionMode,
    host: bool,
    max_ahead_frames: i32,
    native_id_prefix: Option<u32>,
    wol_start: crate::app::match_runtime::session::WolStartTerms,
    /// A co-op continuation's carry-over record (`0x004C6210` after
    /// Start_Scenario); a first start restores none.
    carry_over: Option<CampaignCarryOver>,
}

/// The verified map and admitted rules for a network campaign. The source
/// and path belong to this client and never enter the replay blueprint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreparedCampaignStart {
    pub(crate) authored: crate::sim::scenario_bootstrap::AuthoredCampaignStart,
    /// The exact map House for each admitted command source, in source order.
    pub(crate) primary_houses: Vec<String>,
    pub(crate) map_source: crate::map::source::LoadedMapSource,
    pub(crate) map_effective_digest: String,
    pub(crate) map_name: String,
    pub(crate) local_source: Option<i8>,
    pub(crate) game_speed: i32,
}

impl PreparedNetworkStartup {
    pub(crate) fn new(
        launch: SkirmishLaunchSession,
        seed: u32,
        mode: crate::app::match_runtime::session::SessionMode,
        host: bool,
        max_ahead_frames: i32,
        wol_start: crate::app::match_runtime::session::WolStartTerms,
    ) -> Self {
        let native_id_prefix =
            crate::sim::native_identity::network_native_id_prefix(mode, launch.humans.len());
        Self {
            launch,
            authored_campaign: None,
            local_map_source: None,
            local_map_effective_digest: None,
            local_random_map_preview: None,
            seed: MatchSeed {
                value: seed,
                source: MatchSeedSource::AgreedNetworkHost,
                seed_authority_certifying: false,
            },
            mode,
            host,
            max_ahead_frames,
            native_id_prefix,
            wol_start,
            carry_over: None,
        }
    }

    pub(crate) fn with_carry_over(mut self, record: CampaignCarryOver) -> Self {
        self.carry_over = Some(record);
        self
    }

    pub(crate) fn with_campaign(mut self, start: PreparedCampaignStart) -> Self {
        self.authored_campaign = Some(start);
        // Map House and Super constructors spend the prefix during the load.
        self.native_id_prefix = None;
        self
    }

    pub(crate) fn authored_campaign(&self) -> Option<&PreparedCampaignStart> {
        self.authored_campaign.as_ref()
    }

    pub(crate) fn with_local_map_source(
        mut self,
        source: Option<crate::map::source::LoadedMapSource>,
        effective_digest: Option<String>,
    ) -> Self {
        self.local_map_source = source;
        self.local_map_effective_digest = effective_digest;
        self
    }

    pub(crate) fn with_random_map_preview(
        mut self,
        source: Option<crate::map::source::LoadedMapSource>,
    ) -> Self {
        self.local_random_map_preview = source;
        self
    }
    pub(crate) fn random_map_preview(&self) -> Option<&crate::map::source::LoadedMapSource> {
        self.local_random_map_preview.as_ref()
    }
    pub(crate) fn effective_map_digest(&self) -> Option<&str> {
        self.authored_campaign()
            .map(|start| start.map_effective_digest.as_str())
            .or(self.local_map_effective_digest.as_deref())
    }
    pub(crate) fn local_map_source(&self) -> Option<&crate::map::source::LoadedMapSource> {
        self.authored_campaign()
            .map(|start| &start.map_source)
            .or(self.local_map_source.as_ref())
    }

    pub(crate) fn carry_over(&self) -> Option<&CampaignCarryOver> {
        self.carry_over.as_ref()
    }

    pub(crate) fn max_ahead_frames(&self) -> i32 {
        self.max_ahead_frames
    }
    pub(crate) fn native_id_prefix(&self) -> Option<u32> {
        self.native_id_prefix
    }
    pub(crate) fn wol_start(&self) -> &crate::app::match_runtime::session::WolStartTerms {
        &self.wol_start
    }
    pub fn launch_session(&self) -> &SkirmishLaunchSession {
        &self.launch
    }
    pub(crate) fn mode(&self) -> crate::app::match_runtime::session::SessionMode {
        self.mode
    }
    pub(crate) fn host(&self) -> bool {
        self.host
    }
}

/// Explicit campaign settings, from a client or the process speed-control flag.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CampaignLaunchOptions {
    pub game_speed: i32,
    pub build_off_ally: bool,
    /// External client map root, independent of the retail asset directory.
    pub scenario_directory: Option<std::path::PathBuf>,
}

impl CampaignLaunchOptions {
    /// Main_Tick55D767..55D796 skips the ordinary speed2 store when the
    /// Parse_Command_Line52F94B flag is set. Reuse the existing launch path;
    /// ScenarioSession remains the only live game-speed owner.
    pub(crate) fn for_speed_control(enabled: bool, game_speed: i32) -> Option<Self> {
        enabled.then(|| Self {
            game_speed,
            build_off_ally: crate::sim::game_options::GameOptions::default().build_off_ally,
            scenario_directory: None,
        })
    }

    fn loose_scenario_file(&self, scenario: &str) -> Option<String> {
        let path = self.scenario_directory.as_ref()?.join(scenario);
        let name = path.to_str()?;
        crate::util::file_system::is_file(&path).then(|| name.to_owned())
    }
}

/// StartScenario683B26..683B62 copies Scenario+125C and uppercases it via
/// 7DCFC4 before the exact-case MISSIONMD reads (original gamemd.exe).
/// Keep the supplied physical spelling for case-sensitive loose-file lookup.
///
/// VERA: the name comes from game data (a campaign list, a map's
/// `NextScenario`, MapSelect, a saved game, a launch file), and Start_Scenario
/// hands it to the file system as it is. Here only a logical name starts a
/// mission, so none of them can make the game open a host path or a network
/// share; `None` for any other.
fn prepare_campaign_scenario(
    scenario: &str,
    campaign: &crate::rules::campaigns::CampaignDefinition,
    launch_options: Option<&CampaignLaunchOptions>,
) -> Option<(String, Option<String>)> {
    // Start_Scenario683AC8..683AF8: a null/empty requested name falls back
    // to the retained campaign entry, including after failed MapSelect.
    let scenario = if scenario.is_empty() {
        campaign.scenario()
    } else {
        scenario
    };
    if !crate::map::source::is_logical_name(scenario) {
        log::warn!("Campaign scenario {scenario:?} is not a map name and is not opened");
        return None;
    }
    let logical_name = scenario.to_ascii_uppercase();
    let source = launch_options
        .and_then(|options| options.loose_scenario_file(scenario))
        .or_else(|| (logical_name != scenario).then(|| scenario.to_owned()));
    Some((logical_name, source))
}

/// Start_Scenario683AB0's immutable campaign-array selection, separate from
/// multiplayer slot staging. Full_Init686B6A uses Options.Difficulty literally.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedCampaignStartup {
    campaign_index: usize,
    campaign: crate::rules::campaigns::CampaignDefinition,
    /// Scenario+0x125C, the map Start_Scenario reads. A new campaign copies
    /// the campaign's Scenario (`0x00683AF8`); a won mission passes the map
    /// MapSelect chose (`0x00685C4E`).
    scenario: String,
    /// Physical source spelling when it differs from the logical name, or
    /// the resolved external path. Metadata and continuation use `scenario`.
    scenario_file: Option<String>,
    /// Scenario+0x1254 for this start. A new campaign starts after
    /// BattleControlTerminated6865B0, whose reset matches the ctor's 1; a won
    /// mission increments it before Start_Scenario (`0x00685AAF`).
    mission_counter: i32,
    difficulty: i32,
    launch_options: Option<CampaignLaunchOptions>,
    /// The carry-over record `0x004C6210` writes back once Start_Scenario
    /// has read the map. A new campaign (Main__PrepareSession) never calls
    /// it; a won mission, the restart and the defeat retry all do.
    carry_over: Option<CampaignCarryOver>,
    /// Start_Scenario's DL argument (`0x00683ABC`), tested at `0x00683D9C`
    /// before the post-read `[Basic] Action` movie (Scenario+0x1444). A new
    /// campaign (`0x0052E70C`) and a won mission (`0x00685C45`) pass 1; the
    /// restart (`0x006864ED`) and the defeat retry (`0x00686287`) pass 0.
    /// The pre-read Intro/Brief movie does not depend on it.
    play_action_movie: bool,
    pub seed: MatchSeed,
}

impl PreparedCampaignStartup {
    /// Rebuild only the app's continuation inputs after an admitted saved
    /// world. Native ScenarioLoad689470 retains index+34CC, name+125C and
    /// counter+1254; ContentLoad67E804 restores the incoming carry-over.
    /// The re-read process catalog supplies the same indexed Campaign entry.
    ///
    /// VERA: a saved game is a file from a stranger, so it names no file
    /// location. Its map name must be a logical name, and its loose map path
    /// is used only inside a folder this game already reads maps from
    /// (`LoadedMapSource::restored_loose_lookup`): `launch_dir`, the outgoing
    /// campaign's external folder, or the game's own. A save cannot make
    /// Restart or the next mission open a network share or a device.
    pub(crate) fn from_saved_scenario(
        campaigns: &crate::rules::campaigns::CampaignRegistry,
        simulation: &Simulation,
        source: &crate::map::source::LoadedMapSource,
        ra2_dir: Option<&std::path::Path>,
        launch_dir: Option<&std::path::Path>,
    ) -> Option<Self> {
        let session = simulation.session();
        if session.game_mode_nonzero() {
            return None;
        }
        if !crate::map::source::is_logical_name(session.map_name()) {
            log::warn!(
                "Saved campaign map {:?} is not a map name; Restart and the next mission are off",
                session.map_name()
            );
            return None;
        }
        let campaign_index = usize::try_from(session.campaign_index()?).ok()?;
        let campaign = campaigns.get(campaign_index)?.clone();
        let (scenario_file, scenario_directory) = source.restored_loose_lookup(ra2_dir, launch_dir);
        Some(Self {
            campaign_index,
            campaign,
            scenario: session.map_name().to_owned(),
            scenario_file: scenario_file.map(str::to_owned),
            mission_counter: session.campaign_mission_counter()?,
            difficulty: session.campaign_house_difficulties()?.0 as i32,
            // Reuse the saved session's existing options authority. A loose
            // external campaign also needs its source directory on continuation.
            launch_options: Some(CampaignLaunchOptions {
                game_speed: session.game_options().game_speed,
                build_off_ally: session.game_options().build_off_ally,
                scenario_directory: scenario_directory.map(std::path::Path::to_path_buf),
            }),
            carry_over: session.campaign_restart_carry_over(),
            play_action_movie: false,
            seed: MatchSeed {
                value: session.seed() as u32,
                source: MatchSeedSource::SavedScenario,
                seed_authority_certifying: false,
            },
        })
    }

    pub fn campaign_index(&self) -> usize {
        self.campaign_index
    }

    pub fn campaign(&self) -> &crate::rules::campaigns::CampaignDefinition {
        &self.campaign
    }

    pub fn scenario(&self) -> &str {
        &self.scenario
    }

    pub fn selected_map_file(&self) -> &str {
        self.scenario_file.as_deref().unwrap_or(&self.scenario)
    }

    /// The folder this campaign's loose maps are read from besides the game's:
    /// an external launch's, or the saved map's after a load.
    pub(crate) fn scenario_directory(&self) -> Option<&std::path::Path> {
        self.launch_options.as_ref()?.scenario_directory.as_deref()
    }

    pub fn mission_counter(&self) -> i32 {
        self.mission_counter
    }

    pub fn difficulty(&self) -> i32 {
        self.difficulty
    }

    /// Only explicit external overrides replace the existing campaign defaults.
    pub fn game_options(&self) -> Option<crate::sim::game_options::GameOptions> {
        self.launch_options
            .as_ref()
            .map(|options| crate::sim::game_options::GameOptions {
                game_speed: options.game_speed,
                build_off_ally: options.build_off_ally,
                ..Default::default()
            })
    }

    pub fn carry_over(&self) -> Option<&CampaignCarryOver> {
        self.carry_over.as_ref()
    }

    pub fn plays_action_movie(&self) -> bool {
        self.play_action_movie
    }

    /// Start_Scenario for the same mission: the in-battle Restart
    /// (`0x006863E0`, call at `0x006864FC`) and the defeat retry (`0x00686296`)
    /// both pass Scenario+0x125C, DL 0 and the retained campaign index
    /// Scenario+0x34CC. Neither path touches the mission counter
    /// Scenario+0x1254 or saves the carry-over record, so the record
    /// `0x004C6210` writes back afterwards (`0x00686506`, `0x0068639A`) is the
    /// one this mission started from. Without a won mission the record is the
    /// one `0x004C6140` cleared at session preparation (`0x0052DF0E`): every
    /// global false and the player's difficulty row 1, which SetDifficulty
    /// puts on the player House whatever Options.Difficulty says. The
    /// computer rows and the session's Options.Difficulty are read again.
    pub fn restart(
        &self,
        difficulty: i32,
        game_speed: i32,
        clock: &mut impl MatchSeedClock,
    ) -> Self {
        Self {
            campaign_index: self.campaign_index,
            campaign: self.campaign.clone(),
            scenario: self.scenario.clone(),
            scenario_file: self.scenario_file.clone(),
            mission_counter: self.mission_counter,
            difficulty,
            launch_options: self
                .launch_options
                .as_ref()
                .map(|options| CampaignLaunchOptions {
                    game_speed,
                    ..options.clone()
                }),
            carry_over: Some(self.carry_over.unwrap_or(CampaignCarryOver::CLEARED)),
            play_action_movie: false,
            seed: read_match_seed(clock),
        }
    }

    /// The won mission's Start_Scenario (`0x00685C4E`): the same campaign
    /// index, the map MapSelect chose, the incremented mission counter and
    /// the record `0x004C6180` saved from the won mission. Options.Difficulty
    /// is read again for the session; the player House's row comes from the
    /// record. `None`, with no seed read, when the chosen name is not a map
    /// name ([`prepare_campaign_scenario`]).
    pub fn next_mission(
        &self,
        scenario: String,
        mission_counter: i32,
        difficulty: i32,
        game_speed: i32,
        carry_over: CampaignCarryOver,
        clock: &mut impl MatchSeedClock,
    ) -> Option<Self> {
        let (scenario, scenario_file) =
            prepare_campaign_scenario(&scenario, &self.campaign, self.launch_options.as_ref())?;
        Some(Self {
            campaign_index: self.campaign_index,
            campaign: self.campaign.clone(),
            scenario_file,
            scenario,
            mission_counter,
            difficulty,
            launch_options: self
                .launch_options
                .as_ref()
                .map(|options| CampaignLaunchOptions {
                    game_speed,
                    ..options.clone()
                }),
            carry_over: Some(carry_over),
            play_action_movie: true,
            seed: read_match_seed(clock),
        })
    }
}

pub fn prepare_campaign_startup(
    campaigns: &crate::rules::campaigns::CampaignRegistry,
    name: &str,
    difficulty: i32,
    scenario: Option<&str>,
    launch_options: Option<CampaignLaunchOptions>,
    clock: &mut impl MatchSeedClock,
) -> Option<PreparedCampaignStartup> {
    let campaign_index = campaigns.find_index(name)?;
    prepare_campaign_startup_at_index(
        campaigns,
        campaign_index,
        difficulty,
        scenario,
        launch_options,
        clock,
    )
}

/// Retain the caller's catalog identity independently of an explicit filename.
/// Original683AC8..683AFE uses the record's Scenario only for a null/empty
/// filename;683B0A stores the supplied index. See campaign_start_prefix.json.
/// Named shell starts and external starts share this constructor and consume
/// the seed only after checked catalog admission succeeds.
pub(crate) fn prepare_campaign_startup_at_index(
    campaigns: &crate::rules::campaigns::CampaignRegistry,
    campaign_index: usize,
    difficulty: i32,
    scenario: Option<&str>,
    launch_options: Option<CampaignLaunchOptions>,
    clock: &mut impl MatchSeedClock,
) -> Option<PreparedCampaignStartup> {
    let campaign = campaigns.get(campaign_index)?.clone();
    let (scenario, scenario_file) = prepare_campaign_scenario(
        scenario.unwrap_or_default(),
        &campaign,
        launch_options.as_ref(),
    )?;
    Some(PreparedCampaignStartup {
        campaign_index,
        // External mission selectors can enter a later authored scenario while
        // retaining the selected campaign's final movie and continuation identity.
        scenario_file,
        scenario,
        campaign,
        mission_counter: 1,
        difficulty,
        launch_options,
        carry_over: None,
        play_action_movie: true,
        seed: read_match_seed(clock),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LoadingStartup {
    Network(std::sync::Arc<PreparedNetworkStartup>),
    /// Exact immutable recorded launch. The diagnostics owner retains and
    /// validates its log; this variant never installs a live network service.
    Replay {
        launch: Box<crate::sim::scenario_bootstrap::MatchLaunchDescriptor>,
        seed: MatchSeed,
        game_mode: ScenarioGameMode,
        native_id_prefix: u32,
    },
    AuthoredCampaignReplay {
        start: Box<crate::sim::scenario_bootstrap::AuthoredCampaignStart>,
        logical_map: String,
        local_map: String,
        map_sha256: String,
        seed: MatchSeed,
        game_mode: ScenarioGameMode,
        game_speed: i32,
        native_id_prefix: u32,
    },
    Campaign(PreparedCampaignStartup),
    Accepted(PreparedMatchStartup),
    UnverifiedLegacy {
        session: SkirmishLaunchSession,
        seed: MatchSeed,
    },
    Generic {
        selected_map_file: String,
    },
}

/// The credential-free rule and execution choices for an admitted startup.
/// The startup keeps ownership of map, seed, room state and transport.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct MatchSpec {
    topology: ExecutionTopology,
    rules: RulePolicy,
}

impl MatchSpec {
    pub(crate) const fn topology(self) -> ExecutionTopology {
        self.topology
    }

    pub(crate) const fn rules(self) -> RulePolicy {
        self.rules
    }
}

impl LoadingStartup {
    pub(crate) fn authored_campaign(
        &self,
    ) -> Option<crate::sim::scenario_bootstrap::AuthoredCampaignLaunch<'_>> {
        let (start, local_source) = match self {
            Self::Network(network) => {
                let start = network.authored_campaign()?;
                (&start.authored, start.local_source)
            }
            Self::AuthoredCampaignReplay { start, .. } => (start.as_ref(), Some(0)),
            _ => return None,
        };
        Some(crate::sim::scenario_bootstrap::AuthoredCampaignLaunch {
            start,
            mode: self.session_mode(),
            local_source,
        })
    }

    pub(crate) fn authored_campaign_speed(&self) -> Option<i32> {
        match self {
            Self::Network(startup) => startup.authored_campaign().map(|start| start.game_speed),
            Self::AuthoredCampaignReplay { game_speed, .. } => Some(*game_speed),
            _ => None,
        }
    }

    pub(crate) fn is_replay(&self) -> bool {
        matches!(
            self,
            Self::Replay { .. } | Self::AuthoredCampaignReplay { .. }
        )
    }

    pub(crate) fn match_spec(&self) -> MatchSpec {
        let mut spec =
            self.match_spec_for_construction(self.session_mode().bootstrap_rule_profile());
        if let Self::Replay { launch, .. } = self
            && let Some(policy) = launch.rule_policy_override()
        {
            spec.rules = policy;
        }
        spec
    }

    /// The admitted map family controls construction even when a replay's
    /// recorded session mode is different. Execution still follows the record.
    pub(crate) fn match_spec_for_construction(&self, profile: BootstrapRuleProfile) -> MatchSpec {
        let class = self.launch_session().map(|launch| launch.mode.class);
        let mode = self.session_mode();
        if let Some(authored) = self.authored_campaign() {
            return MatchSpec {
                topology: mode.execution_topology(),
                rules: authored.start.rule_policy(),
            };
        }
        let rule_override = match self {
            Self::Replay { launch, .. } => launch.rule_policy_override(),
            Self::Network(_)
            | Self::AuthoredCampaignReplay { .. }
            | Self::Campaign(_)
            | Self::Accepted(_)
            | Self::UnverifiedLegacy { .. }
            | Self::Generic { .. } => None,
        };
        MatchSpec {
            topology: mode.execution_topology(),
            rules: rule_override
                .map(|policy| policy.with_bootstrap(profile))
                .unwrap_or_else(|| mode.stock_rule_policy_for_bootstrap(profile, class)),
        }
    }

    /// The match seed of a typed startup, which `ScenarioBootstrapRng::new`
    /// seeds the load's Scenario and Main streams with.
    pub(crate) fn match_seed(&self) -> Option<u32> {
        match self {
            Self::Network(startup) => Some(startup.seed.value),
            Self::Replay { seed, .. }
            | Self::AuthoredCampaignReplay { seed, .. }
            | Self::UnverifiedLegacy { seed, .. } => Some(seed.value),
            Self::Campaign(startup) => Some(startup.seed.value),
            Self::Accepted(startup) => Some(startup.seed.value),
            Self::Generic { .. } => None,
        }
    }

    pub(crate) fn native_id_prefix(&self) -> Option<u32> {
        match self {
            Self::Network(startup) => startup.native_id_prefix(),
            Self::Replay {
                native_id_prefix, ..
            } => Some(*native_id_prefix),
            _ => None,
        }
    }

    pub(crate) fn physical_map_file(&self) -> &str {
        if let Self::AuthoredCampaignReplay { local_map, .. } = self {
            return local_map;
        }
        if let Self::Network(startup) = self
            && let Some(crate::map::source::LoadedMapSource::Loose { path, .. }) =
                startup.local_map_source()
            && let Some(path) = path.to_str()
        {
            return path;
        }
        self.selected_map_file()
    }

    pub fn selected_map_file(&self) -> &str {
        match self {
            Self::Network(startup) => startup
                .authored_campaign()
                .map(|start| start.map_name.as_str())
                .unwrap_or_else(|| {
                    startup
                        .launch
                        .selected_map_file
                        .as_deref()
                        .expect("accepted network map")
                }),
            Self::Campaign(startup) => startup.selected_map_file(),
            Self::Accepted(startup) => startup.session.selected_map_file(),
            Self::Replay { launch, .. } => launch
                .session()
                .selected_map_file
                .as_deref()
                .expect("validated replay map"),
            Self::AuthoredCampaignReplay { logical_map, .. } => logical_map,
            Self::UnverifiedLegacy { session, .. } => {
                session.selected_map_file.as_deref().unwrap_or("auto")
            }
            Self::Generic { selected_map_file } => selected_map_file,
        }
    }

    /// VERA: the AI INI this load reads (`retail_sources::match_ai_file`):
    /// the original game's for a network match that seats no computer. A
    /// launch says whether it seats one
    /// (`SkirmishLaunchSession::seats_computers`); a load without a launch,
    /// a campaign's or a campaign room's, always does. The room's rules
    /// agreement answers alike (`MatchSession::network_seats_computers`).
    pub(crate) fn ai_file(
        &self,
        assets: &crate::assets::asset_manager::AssetManager,
    ) -> crate::rules::retail_sources::AiFile {
        crate::rules::retail_sources::match_ai_file(
            assets,
            self.session_mode().is_network(),
            self.launch_session()
                .is_none_or(SkirmishLaunchSession::seats_computers),
        )
    }

    pub fn launch_session(&self) -> Option<&SkirmishLaunchSession> {
        match self {
            Self::Network(startup) => startup
                .authored_campaign()
                .is_none()
                .then(|| startup.launch_session()),
            Self::Campaign(_) | Self::AuthoredCampaignReplay { .. } => None,
            Self::Accepted(startup) => Some(startup.session.launch_session()),
            Self::Replay { launch, .. } => Some(launch.session()),
            Self::UnverifiedLegacy { session, .. } => Some(session),
            Self::Generic { .. } => None,
        }
    }

    pub fn accepted(&self) -> Option<&PreparedMatchStartup> {
        match self {
            Self::Accepted(startup) => Some(startup),
            Self::Network(_)
            | Self::Campaign(_)
            | Self::Replay { .. }
            | Self::AuthoredCampaignReplay { .. }
            | Self::UnverifiedLegacy { .. }
            | Self::Generic { .. } => None,
        }
    }

    pub fn campaign(&self) -> Option<&PreparedCampaignStartup> {
        match self {
            Self::Campaign(startup) => Some(startup),
            _ => None,
        }
    }

    /// The `GameMode` admitted before construction: campaign 0 (`0x0052E175`),
    /// offline Battle 5 (`0x0052E10F`), or the exact network/recorded mode.
    /// A generic map load uses skirmish mode; the admitted map family
    /// separately sets the rules for fresh construction.
    pub(crate) fn session_mode(&self) -> crate::app::match_runtime::session::SessionMode {
        use crate::app::match_runtime::session::SessionMode;
        match self {
            Self::Network(startup) => startup.mode(),
            Self::Replay { game_mode, .. } | Self::AuthoredCampaignReplay { game_mode, .. } => {
                *game_mode
            }
            Self::Campaign(_) => SessionMode::Campaign,
            Self::Accepted(_) | Self::UnverifiedLegacy { .. } | Self::Generic { .. } => {
                SessionMode::Skirmish
            }
        }
    }

    #[cfg(test)]
    pub fn seed_or_else(&self, unverified_fallback: impl FnOnce() -> u32) -> u32 {
        match self {
            Self::Network(startup) => startup.seed.value,
            Self::Campaign(startup) => startup.seed.value,
            Self::Accepted(startup) => startup.seed.value,
            Self::Replay { seed, .. } => seed.value,
            Self::AuthoredCampaignReplay { seed, .. } => seed.value,
            Self::UnverifiedLegacy { seed, .. } => seed.value,
            Self::Generic { .. } => unverified_fallback(),
        }
    }
}

/// Classify without reading entropy or mutating the supplied UI/session value.
pub fn classify_startup_session(session: &SkirmishLaunchSession) -> StartupSessionClassification {
    let Some(selected_map) = session.selected_map_file.as_deref() else {
        return StartupSessionClassification::UnverifiedLegacy(UnverifiedStartupReason::MissingMap);
    };
    let trimmed_map = selected_map.trim();
    if trimmed_map.is_empty()
        || trimmed_map.len() != selected_map.len()
        || trimmed_map.eq_ignore_ascii_case("auto")
    {
        return StartupSessionClassification::UnverifiedLegacy(UnverifiedStartupReason::MissingMap);
    }

    let mode = &session.mode;
    let is_stock_battle =
        mode.id == crate::skirmish_launch::BATTLE_MODE_ID && mode.matching_stock().is_some();
    if !is_stock_battle {
        return StartupSessionClassification::UnverifiedLegacy(
            UnverifiedStartupReason::NonBattleMode,
        );
    }
    if session.opponents.len() > SKIRMISH_AI_SLOT_COUNT {
        return StartupSessionClassification::UnverifiedLegacy(
            UnverifiedStartupReason::InvalidActiveAiCount {
                count: session.opponents.len(),
            },
        );
    }
    if session.humans.len() != 1
        || session.local_player_index != 0
        || session.humans[0].is_observer()
    {
        return StartupSessionClassification::UnverifiedLegacy(
            UnverifiedStartupReason::IncompleteLedger,
        );
    }
    if session.local_player().name.is_empty() {
        return StartupSessionClassification::UnverifiedLegacy(
            UnverifiedStartupReason::IncompleteLedger,
        );
    }
    if session.local_slot().country_random {
        return StartupSessionClassification::UnverifiedLegacy(
            UnverifiedStartupReason::RandomLocalCountry,
        );
    }
    if session.local_slot().color_random {
        return StartupSessionClassification::UnverifiedLegacy(
            UnverifiedStartupReason::RandomLocalColor,
        );
    }
    if matches!(
        session.local_slot().start_position,
        LaunchStartPosition::Auto
    ) {
        return StartupSessionClassification::UnverifiedLegacy(
            UnverifiedStartupReason::AutoLocalStart,
        );
    }
    for (index, slot) in session.opponents.iter().enumerate() {
        if slot.country_random {
            return StartupSessionClassification::UnverifiedLegacy(
                UnverifiedStartupReason::RandomAiCountry { index },
            );
        }
        if slot.color_random {
            return StartupSessionClassification::UnverifiedLegacy(
                UnverifiedStartupReason::RandomAiColor { index },
            );
        }
        if matches!(slot.start_position, LaunchStartPosition::Auto) {
            return StartupSessionClassification::UnverifiedLegacy(
                UnverifiedStartupReason::AutoAiStart { index },
            );
        }
    }

    StartupSessionClassification::AcceptedExplicitFixedBattle(Box::new(AcceptedBattleSession {
        launch: session.clone(),
    }))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MatchCorrelationError {
    #[error("match correlation space is exhausted")]
    Exhausted,
}

/// Allocate a process-lifetime identity. Zero and wrapping reuse are forbidden.
pub fn allocate_match_correlation(
    next: &mut u64,
) -> Result<MatchCorrelationId, MatchCorrelationError> {
    if *next == 0 || *next == u64::MAX {
        return Err(MatchCorrelationError::Exhausted);
    }
    let correlation = MatchCorrelationId(*next);
    *next += 1;
    Ok(correlation)
}

pub trait MatchSeedClock {
    fn low_u32(&mut self) -> u32;
    fn source(&self) -> MatchSeedSource;
    fn seed_authority_certifying(&self) -> bool;
}

#[derive(Default)]
pub struct OrdinaryMatchSeedClock;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetTickCount() -> u32;
}

impl MatchSeedClock for OrdinaryMatchSeedClock {
    fn low_u32(&mut self) -> u32 {
        #[cfg(windows)]
        {
            // SAFETY: GetTickCount takes no arguments and returns the native
            // elapsed-millisecond authority directly as an unsigned 32-bit word.
            unsafe { GetTickCount() }
        }
        #[cfg(not(windows))]
        {
            use crate::util::time::{SystemTime, UNIX_EPOCH};
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default();
            now.subsec_nanos() ^ (now.as_secs() as u32).rotate_left(16)
        }
    }

    fn source(&self) -> MatchSeedSource {
        #[cfg(windows)]
        {
            MatchSeedSource::WindowsGetTickCount
        }
        #[cfg(not(windows))]
        {
            MatchSeedSource::NonWindowsDevelopmentFallback
        }
    }

    fn seed_authority_certifying(&self) -> bool {
        cfg!(windows)
    }
}

pub fn prepare_match_startup(
    correlation: MatchCorrelationId,
    session: AcceptedBattleSession,
    clock: &mut impl MatchSeedClock,
) -> PreparedMatchStartup {
    PreparedMatchStartup {
        correlation,
        seed: read_match_seed(clock),
        session,
    }
}

/// Read one ordinary offline match seed without coupling the caller to the
/// accepted-startup receipt path. Every successful Skirmish Start uses this
/// same authority, including sessions that still have noncertifying mechanics.
pub fn read_match_seed(clock: &mut impl MatchSeedClock) -> MatchSeed {
    MatchSeed {
        value: clock.low_u32(),
        source: clock.source(),
        seed_authority_certifying: clock.seed_authority_certifying(),
    }
}

pub struct RustL0Observation<'a> {
    pub startup: &'a PreparedMatchStartup,
    pub simulation: &'a Simulation,
    pub active_correlation: MatchCorrelationId,
    pub prior_receipt: Option<&'a RustL0Receipt>,
    pub screen_is_loading: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustL0Receipt {
    pub correlation: MatchCorrelationId,
    pub seed: u32,
    pub seed_source: MatchSeedSource,
    pub seed_authority_certifying: bool,
    pub session: AcceptedBattleSession,
    pub tick: u64,
    pub total_sim_ms: u64,
    pub binary_frame: u32,
    pub rngs: SimulationRngState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RustL0Error {
    #[error("a Rust L0 receipt already exists")]
    DuplicateReceipt,
    #[error("loaded startup correlation does not match the active attempt")]
    CorrelationMismatch,
    #[error("installed simulation seed does not match the prepared startup")]
    SeedMismatch,
    #[error("simulation tick is not zero")]
    NonzeroTick,
    #[error("simulation elapsed time is not zero")]
    NonzeroSimulationTime,
    #[error("simulation binary frame is not zero")]
    NonzeroBinaryFrame,
    #[error("the acknowledgement was not observed on the Loading screen")]
    ScreenNotLoading,
}

impl RustL0Observation<'_> {
    pub fn acknowledge(&self) -> Result<RustL0Receipt, RustL0Error> {
        if self.prior_receipt.is_some() {
            return Err(RustL0Error::DuplicateReceipt);
        }
        if self.startup.correlation != self.active_correlation {
            return Err(RustL0Error::CorrelationMismatch);
        }
        if self.simulation.session().seed() != u64::from(self.startup.seed.value) {
            return Err(RustL0Error::SeedMismatch);
        }
        if self.simulation.session().tick() != 0 {
            return Err(RustL0Error::NonzeroTick);
        }
        if self.simulation.session().total_sim_ms() != 0 {
            return Err(RustL0Error::NonzeroSimulationTime);
        }
        if self.simulation.session().binary_frame() != 0 {
            return Err(RustL0Error::NonzeroBinaryFrame);
        }
        if !self.screen_is_loading {
            return Err(RustL0Error::ScreenNotLoading);
        }

        Ok(RustL0Receipt {
            correlation: self.startup.correlation,
            seed: self.startup.seed.value,
            seed_source: self.startup.seed.source,
            seed_authority_certifying: self.startup.seed.seed_authority_certifying,
            session: self.startup.session.clone(),
            tick: self.simulation.session().tick(),
            total_sim_ms: self.simulation.session().total_sim_ms(),
            binary_frame: self.simulation.session().binary_frame(),
            rngs: self.simulation.rng_state(),
        })
    }
}

/// Admission check at the existing app-to-simulation tick boundary.
pub fn accepted_tick_is_admitted(
    startup: Option<&PreparedMatchStartup>,
    receipt: Option<&RustL0Receipt>,
) -> bool {
    let Some(startup) = startup else {
        return true;
    };
    receipt.is_some_and(|receipt| {
        receipt.correlation == startup.correlation
            && receipt.seed == startup.seed.value
            && receipt.seed_source == startup.seed.source
            && receipt.seed_authority_certifying == startup.seed.seed_authority_certifying
            && receipt.session == startup.session
            && receipt.tick == 0
            && receipt.total_sim_ms == 0
            && receipt.binary_frame == 0
    })
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use crate::skirmish_launch::{
        AiDifficulty, LaunchCountry, LaunchTeam, SkirmishAiSlot, SkirmishLaunchMode,
        SkirmishLaunchOptions, SkirmishLocalSlot,
    };

    fn explicit_session() -> SkirmishLaunchSession {
        SkirmishLaunchSession::from_rows(
            SkirmishLaunchMode {
                class: crate::skirmish_modes::MpModeClass::Battle,
                id: 1,
                ui_name_key: "GUI:Battle".into(),
                tooltip_key: "STT:ModeBattle".into(),
                override_file: "MPBattleMD.ini".into(),
                map_filter: "standard".into(),
                random_maps_allowed: true,
                allies_allowed: true,
                must_ally: false,
            },
            Some("DeepFrze.map".into()),
            vec![crate::skirmish_launch::SkirmishHumanSlot {
                raw_slot_index: Some(0),
                name: "Player".into(),
                slot: SkirmishLocalSlot {
                    country: LaunchCountry::America,
                    country_random: false,
                    color_index: 0,
                    color_random: false,
                    start_position: LaunchStartPosition::Position(0),
                    team: LaunchTeam::None,
                },
                role: crate::skirmish_launch::LaunchHumanRole::from_observer(false),
            }],
            0,
            vec![SkirmishAiSlot {
                driver_policy: None,
                raw_slot_index: 1,
                country: LaunchCountry::Russia,
                country_random: false,
                color_index: 1,
                color_random: false,
                start_position: LaunchStartPosition::Position(1),
                team: LaunchTeam::None,
                difficulty: AiDifficulty::Easy,
            }],
            SkirmishLaunchOptions::default(),
        )
    }

    fn accepted_session() -> AcceptedBattleSession {
        match classify_startup_session(&explicit_session()) {
            StartupSessionClassification::AcceptedExplicitFixedBattle(session) => *session,
            other => panic!("explicit fixture was not accepted: {other:?}"),
        }
    }

    struct CountingClock {
        value: u32,
        reads: usize,
    }

    impl MatchSeedClock for CountingClock {
        fn low_u32(&mut self) -> u32 {
            self.reads += 1;
            self.value
        }

        fn source(&self) -> MatchSeedSource {
            MatchSeedSource::Controlled
        }

        fn seed_authority_certifying(&self) -> bool {
            true
        }
    }

    fn prepared(seed: u32) -> PreparedMatchStartup {
        let mut next = 1;
        let correlation = allocate_match_correlation(&mut next).unwrap();
        let mut clock = CountingClock {
            value: seed,
            reads: 0,
        };
        prepare_match_startup(correlation, accepted_session(), &mut clock)
    }

    #[test]
    fn stock_startups_project_rules_independently_of_live_transport() {
        use crate::app::match_runtime::session::{SessionMode, WolStartTerms};

        let offline_startup = LoadingStartup::Accepted(prepared(73));
        assert_eq!(
            offline_startup.session_mode(),
            SessionMode::from_game_mode(5)
        );
        assert!(crate::app::mod_additions::reach(&offline_startup));
        let offline = offline_startup.match_spec();
        assert_eq!(offline.topology(), ExecutionTopology::LocalOffline);
        assert_eq!(offline.rules(), RulePolicy::stock_multiplayer());

        let mut session = explicit_session();
        let replay = |game_mode| LoadingStartup::Replay {
            launch: Box::new(
                crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(
                    session.clone(),
                )
                .unwrap(),
            ),
            seed: MatchSeed {
                value: 73,
                source: MatchSeedSource::SavedScenario,
                seed_authority_certifying: false,
            },
            game_mode,
            native_id_prefix: 0,
        };
        let wol_replay = replay(SessionMode::Wol);
        assert_eq!(
            wol_replay.match_spec().topology(),
            ExecutionTopology::LockstepNetwork
        );
        assert_eq!(
            wol_replay.match_spec().rules(),
            offline.rules().with_allows_ownerless_terrain_logic(false)
        );

        let mode_zero_replay = replay(SessionMode::Campaign);
        assert_eq!(
            mode_zero_replay.match_spec().rules(),
            RulePolicy::stock_campaign()
        );
        assert_eq!(
            mode_zero_replay.match_spec_for_construction(BootstrapRuleProfile::Multiplayer),
            offline,
            "the recorded mode zero does not turn an authored skirmish map into a campaign"
        );

        session.mode = SkirmishLaunchMode::stock(3).unwrap();
        let network = LoadingStartup::Network(std::sync::Arc::new(PreparedNetworkStartup::new(
            session,
            73,
            SessionMode::Lan,
            true,
            48,
            WolStartTerms::default(),
        )));
        let co_op = network.match_spec();
        assert_eq!(co_op.topology(), ExecutionTopology::LockstepNetwork);
        assert_eq!(co_op.rules().bootstrap(), BootstrapRuleProfile::Multiplayer);
        assert!(co_op.rules().shared_allied_victory());
        assert!(co_op.rules().skips_network_score_dialog());
        assert_eq!(
            co_op.rules(),
            co_op.rules().with_ares_psychic_reveal_exclusions(false)
        );
        assert!(!crate::app::mod_additions::reach(&network));

        let spec = serde_json::to_value(co_op).unwrap();
        assert_eq!(spec.as_object().unwrap().len(), 2);
        assert_eq!(serde_json::from_value::<MatchSpec>(spec).unwrap(), co_op);
    }

    #[test]
    fn saved_campaign_keeps_later_map_identity_for_restart_and_victory() {
        let (campaigns, restored) = restored_campaign("SOV03UMD.MAP");
        let source = crate::map::source::LoadedMapSource::Mix {
            logical_name: "SOV03UMD.MAP".into(),
            source_archive: "mapsmd03.mix".into(),
            entry_id: 0,
            payload_len: 0,
            source_sha256: String::new(),
        };
        let saved = PreparedCampaignStartup::from_saved_scenario(
            &campaigns, &restored, &source, None, None,
        )
        .expect("a later map keeps its saved catalog index");
        assert_eq!(saved.campaign_index(), 1);
        assert_eq!(saved.campaign().id(), "SOV1");
        assert_eq!(saved.scenario(), "SOV03UMD.MAP");
        assert_eq!(saved.mission_counter(), 3);
        let mut admission = crate::app::match_runtime::startup::MatchStartup::default();
        admission.acknowledge_saved_load(Some(saved));
        assert!(admission.restored());
        assert!(admission.admits_exact_step());
        assert!(admission.receipt().is_none());
        assert!(
            !admission.campaign_installed(),
            "no fresh-load receipt is fabricated"
        );
        let saved = admission.installed_campaign().unwrap();
        let mut clock = CountingClock {
            value: 88,
            reads: 0,
        };
        let retry = saved.restart(2, 1, &mut clock);
        assert_eq!(retry.campaign_index(), 1);
        assert_eq!(retry.scenario(), "SOV03UMD.MAP");
        assert_eq!(retry.mission_counter(), 3);
        assert_eq!(retry.carry_over(), Some(&CampaignCarryOver::CLEARED));
        assert!(!retry.plays_action_movie());
        let next = saved
            .next_mission(
                "SOV04UMD.MAP".into(),
                4,
                2,
                1,
                restored.save_campaign_carry_over(),
                &mut clock,
            )
            .unwrap();
        assert_eq!(next.campaign_index(), 1);
        assert_eq!(next.scenario(), "SOV04UMD.MAP");
        assert_eq!(next.mission_counter(), 4);
        assert!(next.plays_action_movie());
        assert_eq!(clock.reads, 2, "only the two fresh starts sample a seed");
    }

    /// The SOV1 campaign's third mission under `map_name`, saved and loaded.
    fn restored_campaign(
        map_name: &str,
    ) -> (crate::rules::campaigns::CampaignRegistry, Simulation) {
        use crate::sim::scenario_session::ScenarioDescriptor;
        use crate::sim::snapshot::GameSnapshot;

        let mut campaigns = crate::rules::campaigns::CampaignRegistry::default();
        campaigns.apply_ini(
            &crate::rules::ini_parser::IniFile::from_str(
                "[Battles]\n0=ALL1\n1=SOV1\n[ALL1]\nCD=2\nScenario=ALL01UMD.MAP\n[SOV1]\nCD=2\nScenario=SOV01UMD.MAP\n",
            ),
            &crate::rules::movies::MovieRegistry::default(),
        );
        let mut sim = Simulation::from_descriptor(&ScenarioDescriptor {
            map_name: map_name.into(),
            seed: 71,
            ..Default::default()
        });
        sim.session_mut_for_test()
            .initialize_campaign_for_test(1, 3, CampaignCarryOver::CLEARED);
        let bytes = GameSnapshot::save(&sim, 0, 0, "SOV03UMD.MAP", 0);
        let restored = GameSnapshot::load(&bytes)
            .unwrap()
            .into_simulation_for_test();
        (campaigns, restored)
    }

    /// A saved game is a file from a stranger: its map path, its mission name
    /// and its next mission's name all reach the file system on Restart or a win.
    #[test]
    fn a_saved_campaign_names_no_file_outside_the_map_folders() {
        use crate::map::source::test_support::TestDirectory;
        use std::path::Path;
        let loose = |path: &Path| crate::map::source::LoadedMapSource::Loose {
            path: path.to_path_buf(),
            payload_len: 0,
            source_sha256: String::new(),
            cache_lease: Default::default(),
        };
        let folder = |label: &str| {
            let folder = TestDirectory::new(label);
            std::fs::create_dir(folder.path().join("Maps")).unwrap();
            folder.write("Maps/sov03umd.map", b"[Map]");
            folder
        };
        let map = |folder: &TestDirectory| folder.path().join("Maps").join("sov03umd.map");
        let (game, client, elsewhere) = (
            folder("saved-campaign-game"),
            folder("saved-campaign-client"),
            folder("saved-campaign-elsewhere"),
        );
        let (campaigns, restored) = restored_campaign("SOV03UMD.MAP");
        let saved = |path: &Path| {
            PreparedCampaignStartup::from_saved_scenario(
                &campaigns,
                &restored,
                &loose(path),
                Some(game.path()),
                Some(client.path()),
            )
            .unwrap()
        };
        // Inside a map folder the saved file is the map; an external launch's
        // folder stays where the next mission is looked for.
        for (inside, next_from) in [
            (map(&game), game.path().join("Maps")),
            (map(&client), client.path().to_path_buf()),
        ] {
            let startup = saved(&inside);
            assert_eq!(startup.selected_map_file(), inside.to_str().unwrap());
            assert_eq!(startup.scenario_directory(), Some(next_from.as_path()));
        }
        // Outside them even a file that exists is only a name to look up.
        let through_parent = game
            .path()
            .join("..")
            .join(elsewhere.path().file_name().unwrap())
            .join("Maps")
            .join("sov03umd.map");
        assert!(through_parent.is_file());
        for outside in [map(&elsewhere), through_parent] {
            let startup = saved(&outside);
            assert_eq!(startup.selected_map_file(), "sov03umd.map", "{outside:?}");
            assert_eq!(startup.scenario_directory(), None, "{outside:?}");
        }
        let share = saved(Path::new(r"\\host\share\sov03umd.map"));
        assert!(crate::map::source::is_logical_name(
            share.selected_map_file()
        ));
        assert_eq!(share.scenario_directory(), None);

        let startup = saved(&map(&game));
        let mut clock = CountingClock { value: 1, reads: 0 };
        for name in [
            r"\\host\share\NEXT.MAP",
            "../NEXT.MAP",
            "/dev/zero",
            "COM1.MAP",
        ] {
            let next =
                startup.next_mission(name.into(), 4, 2, 1, CampaignCarryOver::CLEARED, &mut clock);
            assert!(next.is_none(), "{name}");
        }
        assert_eq!(clock.reads, 0, "a refused name reads no seed");
        let (campaigns, restored) = restored_campaign("../SOV03UMD.MAP");
        assert!(
            PreparedCampaignStartup::from_saved_scenario(
                &campaigns,
                &restored,
                &loose(&map(&game)),
                Some(game.path()),
                None
            )
            .is_none()
        );
    }

    #[test]
    fn saved_loose_campaign_restarts_from_the_current_install() {
        let (campaigns, sim) = restored_campaign("SOV03UMD.MAP");
        let mut clock = CountingClock {
            value: 12,
            reads: 0,
        };
        let install =
            crate::map::source::test_support::TestDirectory::new("saved-campaign-install");
        let mut restart = |path| {
            let source = crate::map::source::LoadedMapSource::Loose {
                path,
                cache_lease: Default::default(),
                payload_len: 12,
                source_sha256: "saved-digest".into(),
            };
            PreparedCampaignStartup::from_saved_scenario(
                &campaigns,
                &sim,
                &source,
                Some(install.path()),
                None,
            )
            .unwrap()
            .restart(1, 1, &mut clock)
        };
        // The physical filename can differ from the campaign's logical name.
        install.write("Physical.map", b"[Map]");
        let original = install.path().join("Physical.map");
        assert_eq!(
            restart(original.clone()).selected_map_file(),
            original.to_str().unwrap()
        );
        let moved = install.path().join("moved");
        std::fs::create_dir(&moved).unwrap();
        std::fs::rename(&original, moved.join("Physical.map")).unwrap();
        let retry = restart(original);
        assert!(moved.join(retry.selected_map_file()).is_file());
        assert_eq!(retry.selected_map_file(), "Physical.map");

        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let path = std::ffi::OsStr::from_bytes(b"/game/\xff/Mission.map");
            assert_eq!(restart(path.into()).selected_map_file(), "Mission.map");
            assert_eq!(
                restart(r"C:\other\Physical.map".into()).selected_map_file(),
                "SOV03UMD.MAP"
            );
        }
    }

    #[test]
    fn speed_control_campaign_preserves_uncapped_speed_through_start_and_restart() {
        use crate::app::frontend::startup_options::consume_retail_switches;
        let ini = crate::rules::ini_parser::IniFile::from_str(
            "[Battles]\n0=ALL1\n[ALL1]\nCD=2\nScenario=ALL01UMD.MAP\n",
        );
        let mut campaigns = crate::rules::campaigns::CampaignRegistry::default();
        campaigns.apply_ini(&ini, &crate::rules::movies::MovieRegistry::default());
        let (ordinary, _) = consume_retail_switches(Vec::new());
        assert!(
            CampaignLaunchOptions::for_speed_control(ordinary.campaign_speed_control, 0).is_none()
        );
        let (enabled, rest) = consume_retail_switches(vec!["-speedcontrol".into()]);
        assert!(rest.is_empty());
        let mut clock = CountingClock {
            value: 731,
            reads: 0,
        };
        let startup = prepare_campaign_startup(
            &campaigns,
            "ALL1",
            1,
            None,
            CampaignLaunchOptions::for_speed_control(enabled.campaign_speed_control, 0),
            &mut clock,
        )
        .unwrap();
        let options = startup.game_options().unwrap();
        assert_eq!(options.game_speed, 0);
        assert_eq!(
            options.build_off_ally,
            crate::sim::game_options::GameOptions::default().build_off_ally
        );
        let restart = startup.restart(1, 4, &mut clock);
        assert_eq!(restart.game_options().unwrap().game_speed, 4);
        assert_eq!(restart.scenario(), startup.scenario());
        assert_eq!(restart.campaign_index(), startup.campaign_index());
        let campaign = LoadingStartup::Campaign(startup).match_spec();
        assert_eq!(campaign.topology(), ExecutionTopology::LocalOffline);
        assert_eq!(
            campaign.rules().bootstrap(),
            crate::rules::rule_policy::BootstrapRuleProfile::Campaign
        );
        assert!(!campaign.rules().shared_allied_victory());
    }

    #[test]
    fn cncnet_campaign_enters_a_selected_mission_and_keeps_continuation_identity() {
        let ini = crate::rules::ini_parser::IniFile::from_str(
            "[Battles]\n0=ALL1\n[ALL1]\nCD=2\nScenario=ALL01UMD.MAP\n",
        );
        let mut campaigns = crate::rules::campaigns::CampaignRegistry::default();
        campaigns.apply_ini(&ini, &crate::rules::movies::MovieRegistry::default());
        let mut clock = CountingClock {
            value: 731,
            reads: 0,
        };
        let startup = prepare_campaign_startup(
            &campaigns,
            "ALL1",
            2,
            Some("ALL04uMd.map"),
            Some(CampaignLaunchOptions {
                game_speed: 2,
                build_off_ally: false,
                scenario_directory: None,
            }),
            &mut clock,
        )
        .expect("campaign admission");
        assert_eq!(startup.scenario(), "ALL04UMD.MAP");
        assert_eq!(startup.selected_map_file(), "ALL04uMd.map");
        assert_eq!(startup.campaign().scenario(), "ALL01UMD.MAP");
        assert_eq!(startup.campaign().id(), "ALL1");
        assert_eq!(startup.campaign().cd(), 2);
        assert_eq!(startup.difficulty(), 2);
        assert_eq!(startup.game_options().unwrap().game_speed, 2);
        assert!(!startup.game_options().unwrap().build_off_ally);
        assert_eq!(startup.seed.value, 731);
        assert_eq!(clock.reads, 1);
        let next = startup
            .next_mission(
                "ALL05uMd.map".into(),
                2,
                2,
                4,
                CampaignCarryOver::CLEARED,
                &mut clock,
            )
            .unwrap();
        assert_eq!(next.campaign_index(), startup.campaign_index());
        assert_eq!(next.campaign(), startup.campaign());
        assert_eq!(next.scenario(), "ALL05UMD.MAP");
        assert_eq!(next.selected_map_file(), "ALL05uMd.map");
        assert_eq!(next.game_options().unwrap().game_speed, 4);
        assert!(!next.game_options().unwrap().build_off_ally);
        assert!(startup.plays_action_movie() && next.plays_action_movie());
        // SkipMapSelect clears FileName on an unmatched destination. Native
        // Start_Scenario683AC8..683AF8 then chooses the retained campaign entry.
        let mut sim = crate::sim::world::Simulation::new();
        sim.set_campaign_map_stage(17);
        let record = sim.save_campaign_carry_over();
        let fallback = next
            .next_mission(String::new(), 3, 2, 4, record, &mut clock)
            .unwrap();
        assert_eq!(fallback.scenario(), "ALL01UMD.MAP");
        assert_eq!(fallback.campaign_index(), startup.campaign_index());
        assert_eq!(fallback.mission_counter(), 3);
        assert_eq!(fallback.carry_over(), Some(&record));
        assert_eq!(fallback.carry_over().unwrap().map_stage(), 17);
    }

    /// Restart and the defeat retry re-enter Start_Scenario with the same
    /// FileName and campaign index, DL 0 and the record this mission
    /// started from; only the seed and Options.Difficulty are read again.
    #[test]
    fn restart_reenters_the_same_mission_without_the_action_movie() {
        let ini = crate::rules::ini_parser::IniFile::from_str(
            "[Battles]\n0=ALL1\n1=SOV1\n[ALL1]\nScenario=ALL01UMD.MAP\n[SOV1]\nScenario=SOV01UMD.MAP\n",
        );
        let mut campaigns = crate::rules::campaigns::CampaignRegistry::default();
        campaigns.apply_ini(&ini, &crate::rules::movies::MovieRegistry::default());
        let mut clock = CountingClock { value: 5, reads: 0 };
        let first = prepare_campaign_startup(&campaigns, "SOV1", 0, None, None, &mut clock)
            .expect("campaign admission");
        let mut won = crate::sim::world::Simulation::new();
        won.set_trigger_variable_for_test(true, 3, true);
        let record = won.save_campaign_carry_over();
        let second = first
            .next_mission("SOV02SMD.MAP".into(), 2, 0, 1, record, &mut clock)
            .unwrap();
        clock.value = 77;
        let restarted = second.restart(2, 1, &mut clock);
        assert_eq!(restarted.campaign_index(), second.campaign_index());
        assert_eq!(restarted.campaign(), second.campaign());
        assert_eq!(restarted.scenario(), "SOV02SMD.MAP");
        assert_eq!(restarted.selected_map_file(), second.selected_map_file());
        assert_eq!(restarted.mission_counter(), 2);
        assert_eq!(restarted.carry_over(), Some(&record));
        assert!(restarted.carry_over().unwrap().globals()[3]);
        assert_eq!(restarted.difficulty(), 2);
        assert_eq!(restarted.seed.value, 77);
        assert!(!restarted.plays_action_movie());
        // A first mission starts without the record but restarts with the
        // one `0x004C6140` cleared, player row 1 included.
        assert_eq!(first.carry_over(), None);
        assert_eq!(
            first.restart(0, 1, &mut clock).carry_over(),
            Some(&CampaignCarryOver::CLEARED)
        );
    }

    fn acknowledge<'a>(
        startup: &'a PreparedMatchStartup,
        simulation: &'a Simulation,
    ) -> Result<RustL0Receipt, RustL0Error> {
        RustL0Observation {
            startup,
            simulation,
            active_correlation: startup.correlation,
            prior_receipt: None,
            screen_is_loading: true,
        }
        .acknowledge()
    }

    #[test]
    fn unresolved_choices_are_unverified() {
        let mut automatic_map = explicit_session();
        automatic_map.selected_map_file = Some("auto".into());
        assert_eq!(
            classify_startup_session(&automatic_map),
            StartupSessionClassification::UnverifiedLegacy(UnverifiedStartupReason::MissingMap)
        );

        let mut session = explicit_session();
        session.local_slot_mut().country_random = true;
        assert_eq!(
            classify_startup_session(&session),
            StartupSessionClassification::UnverifiedLegacy(
                UnverifiedStartupReason::RandomLocalCountry
            )
        );
        session.local_slot_mut().country_random = false;
        session.opponents[0].start_position = LaunchStartPosition::Auto;
        assert_eq!(
            classify_startup_session(&session),
            StartupSessionClassification::UnverifiedLegacy(UnverifiedStartupReason::AutoAiStart {
                index: 0
            })
        );
    }

    #[test]
    fn accepted_loading_seed_preserves_u32_boundaries_through_descriptor_and_simulation() {
        for value in [0, 1, 0x7fff_ffff, u32::MAX] {
            let loading = LoadingStartup::Accepted(prepared(value));
            let mut fallback_calls = 0;
            let descriptor = crate::sim::scenario_session::ScenarioDescriptor {
                seed: loading.seed_or_else(|| {
                    fallback_calls += 1;
                    0xDEAD_BEEF
                }),
                ..Default::default()
            };
            let simulation = Simulation::from_descriptor(&descriptor);
            let expected_rng = crate::sim::rng::SimRng::new(u64::from(value)).logical_state();
            let expected_mapgen = crate::sim::rng::SimRng::new(0).logical_state();

            assert_eq!(fallback_calls, 0);
            assert_eq!(descriptor.seed, value);
            assert_eq!(simulation.session().seed(), u64::from(value));
            assert_eq!(simulation.rng_state().scenario, expected_rng);
            assert_eq!(simulation.rng_state().main, expected_rng);
            assert_eq!(simulation.rng_state().mapgen, expected_mapgen);
        }
    }

    #[test]
    fn rust_l0_receipt_captures_each_rng_stream_exactly() {
        let startup = prepared(7);
        let mut simulation = Simulation::with_seed(7);
        simulation.scenario_rng_mut_for_test().next_u32();
        simulation.main_rng_mut_for_test().next_u32();
        simulation.mapgen_rng_mut_for_test().next_u32();
        let expected = simulation.rng_state();
        let receipt = RustL0Observation {
            startup: &startup,
            simulation: &simulation,
            active_correlation: startup.correlation,
            prior_receipt: None,
            screen_is_loading: true,
        }
        .acknowledge()
        .unwrap();
        assert_eq!(receipt.rngs, expected);
        assert!(!accepted_tick_is_admitted(Some(&startup), None));
        assert!(accepted_tick_is_admitted(Some(&startup), Some(&receipt)));
    }

    #[test]
    fn rust_l0_acknowledges_once_before_in_game() {
        let startup = prepared(11);
        let simulation = Simulation::with_seed(11);
        let receipt = acknowledge(&startup, &simulation).unwrap();
        assert_eq!(receipt.tick, 0);
        assert_eq!(receipt.total_sim_ms, 0);
        assert_eq!(receipt.binary_frame, 0);
        assert_eq!(
            RustL0Observation {
                startup: &startup,
                simulation: &simulation,
                active_correlation: startup.correlation,
                prior_receipt: Some(&receipt),
                screen_is_loading: true,
            }
            .acknowledge(),
            Err(RustL0Error::DuplicateReceipt)
        );
    }

    #[test]
    fn rust_l0_rejects_seed_or_active_correlation_mismatch() {
        let startup = prepared(16);
        let wrong_seed_simulation = Simulation::with_seed(17);
        assert_eq!(
            acknowledge(&startup, &wrong_seed_simulation),
            Err(RustL0Error::SeedMismatch)
        );

        let simulation = Simulation::with_seed(16);
        let mut next = 2;
        let other = allocate_match_correlation(&mut next).unwrap();
        assert_eq!(
            RustL0Observation {
                startup: &startup,
                simulation: &simulation,
                active_correlation: other,
                prior_receipt: None,
                screen_is_loading: true,
            }
            .acknowledge(),
            Err(RustL0Error::CorrelationMismatch)
        );
    }

    #[test]
    fn accepted_tick_rejects_each_mismatched_receipt_field_and_nonzero_clock() {
        let startup = prepared(20);
        let simulation = Simulation::with_seed(20);
        let receipt = acknowledge(&startup, &simulation).unwrap();
        assert!(accepted_tick_is_admitted(Some(&startup), Some(&receipt)));

        let mut rejected = Vec::new();

        let mut candidate = receipt.clone();
        candidate.correlation = MatchCorrelationId(candidate.correlation.get() + 1);
        rejected.push(("correlation", candidate));

        let mut candidate = receipt.clone();
        candidate.seed ^= 1;
        rejected.push(("seed", candidate));

        let mut candidate = receipt.clone();
        candidate.seed_source = MatchSeedSource::WindowsGetTickCount;
        rejected.push(("seed source", candidate));

        let mut candidate = receipt.clone();
        candidate.seed_authority_certifying = false;
        rejected.push(("seed authority", candidate));

        let mut candidate = receipt.clone();
        candidate
            .session
            .launch
            .local_player_mut()
            .name
            .push_str("-other");
        rejected.push(("session", candidate));

        let mut candidate = receipt.clone();
        candidate.tick = 1;
        rejected.push(("tick", candidate));

        let mut candidate = receipt.clone();
        candidate.total_sim_ms = 1;
        rejected.push(("total simulation time", candidate));

        let mut candidate = receipt.clone();
        candidate.binary_frame = 1;
        rejected.push(("binary frame", candidate));

        for (field, candidate) in rejected {
            assert!(
                !accepted_tick_is_admitted(Some(&startup), Some(&candidate)),
                "receipt mutation must be rejected: {field}"
            );
        }
        assert!(accepted_tick_is_admitted(None, None));
    }
}
