//! Stock-offline Scenario prefix before terrain Fill: the House constructor
//! timer passes, the selected start-callback family, and the prefix plan that
//! owns both Gather passes until its draw-free projection.

use super::campaign_player_name;
use super::{
    MatchLaunchDescriptor, NativeStartAssignment,
    native_assign_cooperative_starts_from_preassignment,
    native_assign_launch_starts_from_preassignment,
    native_assign_quick_coop_starts_from_preassignment, native_gather_pre_fill_start_positions,
    native_preassign_launch_start_table,
};
use crate::map::campaign_manifest::CampaignGrant;
use crate::map::campaign_manifest::CampaignRole;
use crate::map::campaign_manifest::CampaignStage;
use crate::map::campaign_manifest::MAX_CAMPAIGN_PLAYERS;
use crate::map::campaign_manifest::ManifestControlScope;
use crate::map::houses::HouseRoster;
use crate::map::map_file::MapFile;
use crate::map::waypoints::Waypoint;
use crate::rules::rule_policy::BootstrapRuleProfile;
use crate::rules::rule_policy::RulePolicy;
use crate::rules::ruleset::RuleSet;
use crate::sim::campaign_carry_over::StageCarryOver;
use crate::sim::control_authority::ControlGrant;
use crate::sim::control_authority::ControlGrantLevel;
use crate::sim::house_state::HouseDifficulty;
use crate::sim::rng::{SimRng, SimRngLogicalState};
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::world::Simulation;
use crate::skirmish_launch::{PreFillHouseRoster, SkirmishLaunchSession};
use std::collections::HashMap;

pub const HOUSE_CONSTRUCTOR_TIMER_MIN: u32 = 450;
pub const HOUSE_CONSTRUCTOR_TIMER_MAX: u32 = 1800;

/// The selected active-retail start callback used by offline noncampaign Full Init.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StockOfflineStartCallbackFamily {
    Battle,
    Cooperative,
}

/// Immutable stock-offline Scenario prefix prepared before terrain Fill.
///
/// This plan is the sole owner of the disposable first House pass, both
/// selected-mode Gather callbacks, the final chooser, the zero-draw reset, and
/// the second House pass. Only the second Gather vector and its retained
/// assignment are projected later; projection is deliberately draw-free.
#[derive(Debug)]
pub struct PreFillScenarioPrefixPlan {
    pub(super) start_header: crate::map::preview::ScenarioStartHeader,
    pub(super) projection: StockOfflinePrefixProjection,
    #[cfg(any(test, feature = "test-support"))]
    first_gathered_starts: Vec<Waypoint>,
    pub(super) first_house_timers: Vec<u32>,
    pub(super) second_house_timers: Vec<u32>,
    scenario_rng_before: SimRngLogicalState,
    scenario_rng_before_fingerprint: u64,
    pub(super) scenario_rng_after: SimRngLogicalState,
    pub(super) scenario_rng_after_cursor: SimRng,
    native_resize_width: u32,
    native_resize_height: u32,
    #[cfg(any(test, feature = "test-support"))]
    rng_checkpoints: ScenarioPrefixRngCheckpoints,
}

/// Paired Scenario-RNG and native numeric-ID prefix for one supported fresh
/// stock-offline Full_Init. Neither half can reach production independently.
#[derive(Debug)]
pub struct BoundStockOfflineFreshPrefixPlan {
    scenario: PreFillScenarioPrefixPlan,
    pub(super) native_ids: crate::sim::native_identity::NativeFreshIdPrefixReceipt,
}

/// Family-specific inputs to the one pre-Fill Simulation handoff. Campaigns
/// construct the authored House array on this owner; they never synthesize an
/// offline start assignment or a draw-free projection of that assignment.
#[expect(
    clippy::large_enum_variant,
    reason = "built once per scenario load and consumed immediately"
)]
pub enum FreshScenarioPrefix<'a> {
    StockOffline(BoundStockOfflineFreshPrefixPlan),
    Campaign {
        native_rules_receipt: crate::rules::process_owner::NativeScenarioRulesReceipt,
        rules: &'a RuleSet,
        map_data: &'a MapFile,
        house_roster: &'a HouseRoster,
        difficulty: i32,
        mission_counter: i32,
        game_options: Option<&'a crate::sim::game_options::GameOptions>,
        campaign_index: i32,
        restart_carry_over: crate::sim::campaign_carry_over::CampaignCarryOver,
        network: Option<AuthoredCampaignLaunch<'a>>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum FreshScenarioPrefixError {
    #[error(transparent)]
    Offline(#[from] PreFillScenarioPrefixPlanError),
    #[error(transparent)]
    Campaign(#[from] crate::sim::scenario_session::CampaignScenarioStartupError),
    #[error(transparent)]
    Identity(#[from] crate::sim::native_identity::NativeIdentityError),
    #[error("invalid authored campaign roster: {0}")]
    AuthoredCampaign(#[source] anyhow::Error),
    #[error("campaign House construction requires an authored [Houses] roster")]
    MissingCampaignHouseRoster,
    #[error("campaign [Houses] repeats the name '{0}'")]
    DuplicateCampaignHouse(String),
    #[error("campaign House construction requires a registered entry0 Country")]
    MissingCampaignCountryRegistry,
    #[error("campaign House {house} requires unsupported Country constructor {country}")]
    UnsupportedCampaignCountryConstructor { house: String, country: String },
    #[error(
        "campaign Super constructors have {allocated} native types but {projected} registry identities"
    )]
    SuperWeaponRegistryMismatch { allocated: usize, projected: usize },
}

/// Deterministic startup facts for the authored House construction path.
/// Source indices are the order of the roles in the manifest.
#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoredCampaignStart {
    pub(crate) rule_policy: RulePolicy,
    pub(crate) roles: Vec<CampaignRole>,
    pub(crate) grants: Vec<CampaignGrant>,
    pub(crate) difficulty: i32,
    pub(crate) mission_counter: i32,
    pub(crate) carry_over: StageCarryOver,
}

/// Transport and local view for one client constructing the shared campaign.
#[derive(Debug, Clone, Copy)]
pub struct AuthoredCampaignLaunch<'a> {
    pub start: &'a AuthoredCampaignStart,
    pub mode: ScenarioGameMode,
    pub local_source: Option<i8>,
}

impl AuthoredCampaignStart {
    pub fn from_stage(
        stage: &CampaignStage,
        stage_index: usize,
        difficulty: i32,
        carry_over: StageCarryOver,
    ) -> anyhow::Result<Self> {
        let mission_counter = i32::try_from(stage_index)?
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("campaign stage index is too large"))?;
        let start = Self {
            rule_policy: stage.rule_policy,
            roles: stage.roles.clone(),
            grants: stage.grants.clone(),
            difficulty,
            mission_counter,
            carry_over,
        };
        start.validate()?;
        Ok(start)
    }

    pub const fn difficulty(&self) -> i32 {
        self.difficulty
    }

    pub const fn rule_policy(&self) -> RulePolicy {
        self.rule_policy
    }

    pub const fn mission_counter(&self) -> i32 {
        self.mission_counter
    }

    pub const fn carry_over(&self) -> &StageCarryOver {
        &self.carry_over
    }

    pub fn roles(&self) -> &[CampaignRole] {
        &self.roles
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.rule_policy.bootstrap() == BootstrapRuleProfile::Campaign
                && !self.rule_policy.counts_player_control_as_human()
                && self.mission_counter > 0,
            "authored campaign startup requires MapHouses, role-based player control, and a positive mission number"
        );
        anyhow::ensure!(
            HouseDifficulty::from_native(self.difficulty).is_some(),
            "invalid authored campaign difficulty {}",
            self.difficulty
        );
        anyhow::ensure!(
            self.roles.len() <= usize::from(MAX_CAMPAIGN_PLAYERS),
            "too many campaign roles"
        );
        let mut roles = std::collections::BTreeMap::new();
        for role in &self.roles {
            anyhow::ensure!(
                !role.name.is_empty()
                    && !role.house.is_empty()
                    && roles
                        .insert(role.name.to_ascii_lowercase(), &role.house)
                        .is_none(),
                "invalid or duplicate campaign role '{}'",
                role.name
            );
        }
        let mut targets = std::collections::BTreeSet::new();
        for grant in &self.grants {
            let role = grant.role.to_ascii_lowercase();
            let target = grant.house.to_ascii_lowercase();
            let primary = roles
                .get(&role)
                .ok_or_else(|| anyhow::anyhow!("unknown campaign role '{}'", grant.role))?;
            anyhow::ensure!(
                !target.is_empty() && !primary.eq_ignore_ascii_case(&target),
                "campaign grant for '{}' requires a distinct target House",
                grant.role
            );
            anyhow::ensure!(
                targets.insert((role, target)),
                "campaign role '{}' repeats its grant for '{}'",
                grant.role,
                grant.house
            );
        }
        self.carry_over.validate()?;
        Ok(())
    }

    /// Resolve the map's retained House name for every authored source.
    pub fn primary_house_names(
        &self,
        roster: &HouseRoster,
        ini: &crate::rules::ini_parser::IniFile,
    ) -> anyhow::Result<Vec<String>> {
        self.primary_houses(roster, ini)?
            .into_iter()
            .map(|index| {
                roster
                    .houses
                    .get(usize::from(index))
                    .map(|house| house.name.clone())
                    .ok_or_else(|| anyhow::anyhow!("campaign source has no starting House"))
            })
            .collect()
    }

    fn authored_house_index(roster: &HouseRoster, name: &str) -> anyhow::Result<u8> {
        roster
            .houses
            .iter()
            .position(|house| house.name.eq_ignore_ascii_case(name))
            .and_then(|index| u8::try_from(index).ok())
            .ok_or_else(|| anyhow::anyhow!("unknown authored House '{name}'"))
    }

    fn primary_houses(
        &self,
        roster: &HouseRoster,
        ini: &crate::rules::ini_parser::IniFile,
    ) -> anyhow::Result<Vec<u8>> {
        if self.roles.is_empty() {
            anyhow::ensure!(!roster.houses.is_empty(), "campaign has no starting House");
            let requested = campaign_player_name(ini);
            let fallback = crate::map::houses::find_house_index(
                roster.houses.iter().map(|house| house.name.as_str()),
                &requested,
            )
            .unwrap_or(0);
            return Ok(vec![u8::try_from(fallback)?]);
        }
        self.roles
            .iter()
            .map(|role| Self::authored_house_index(roster, &role.house))
            .collect()
    }

    fn resolve_roster(
        &self,
        roster: &HouseRoster,
        ini: &crate::rules::ini_parser::IniFile,
    ) -> anyhow::Result<(Vec<u8>, Vec<ControlGrant>)> {
        let sources = self.primary_houses(roster, ini)?;
        let grants = self
            .grants
            .iter()
            .map(|grant| {
                let source = self
                    .roles
                    .iter()
                    .position(|role| role.name.eq_ignore_ascii_case(&grant.role))
                    .ok_or_else(|| anyhow::anyhow!("unknown campaign role '{}'", grant.role))?;
                Ok(ControlGrant {
                    source: i8::try_from(source)?,
                    house: Self::authored_house_index(roster, &grant.house)?,
                    level: match grant.scope {
                        ManifestControlScope::FullControl => ControlGrantLevel::FullControl,
                        ManifestControlScope::UnitsOnly => ControlGrantLevel::UnitsOnly,
                    },
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok((sources, grants))
    }
}

impl AuthoredCampaignLaunch<'_> {
    pub(super) fn admit_sources(
        self,
        sim: &mut Simulation,
        roster: &HouseRoster,
        ini: &crate::rules::ini_parser::IniFile,
    ) -> anyhow::Result<(usize, Vec<ControlGrant>)> {
        self.start.validate()?;
        anyhow::ensure!(
            self.mode.execution_topology().is_lockstep(),
            "an authored network campaign requires a lockstep session"
        );
        let (sources, grants) = self.start.resolve_roster(roster, ini)?;
        // The first source supplies the initial camera for a connection-level
        // watcher. No source or House is assigned to that connection.
        let view_source = usize::try_from(self.local_source.unwrap_or(0))?;
        let view_house = sources
            .get(view_source)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("unknown local campaign source {view_source}"))?;
        sim.install_session_admission(self.mode, None, self.start.rule_policy);
        sim.session
            .install_shared_house_sources(sources, self.local_source);
        if self.local_source.is_none() {
            sim.install_watcher();
        }
        Ok((usize::from(view_house), grants))
    }
}

pub(super) fn finish_campaign_roster(
    sim: &mut Simulation,
    view_house: usize,
    grants: Vec<ControlGrant>,
) -> anyhow::Result<()> {
    for house in sim.houses.in_order() {
        anyhow::ensure!(
            !house.is_human() || !house.multiplay_passive(),
            "campaign role binds passive House '{}'",
            sim.interner.resolve(house.name())
        );
    }
    let view_house = sim.houses.order().get(view_house).copied();
    anyhow::ensure!(
        view_house.is_some(),
        "campaign view House was not constructed"
    );
    sim.session.install_current_house(view_house);
    sim.install_control_grants(grants)?;
    Ok(())
}

/// Draw-free stock-offline state retained after the prefix cursor receipt is
/// consumed.  It can place the final Houses and expose loading markers, but it
/// owns no RNG and cannot replay the prefix.
#[derive(Debug)]
pub struct StockOfflinePrefixProjection {
    /// Raw active Scenario waypoint table after the authored read or accepted
    /// RMG staging copy. Gather may derive fallback cells from this table, but
    /// loading markers and the live Scenario/session owner retain these exact
    /// entries rather than the Gather result or regenerated `.SED` table.
    active_scenario_waypoints: HashMap<u32, Waypoint>,
    final_gathered_starts: Vec<Waypoint>,
    pub(super) assignment: NativeStartAssignment,
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScenarioPrefixRngCheckpoints {
    pub(crate) after_first_house_pass: SimRngLogicalState,
    pub(crate) after_first_gather: SimRngLogicalState,
    pub(crate) after_second_gather_and_chooser: SimRngLogicalState,
    pub(crate) after_zero_draw_reset: SimRngLogicalState,
    pub(crate) after_second_house_pass: SimRngLogicalState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PreFillScenarioPrefixPlanError {
    #[error(
        "pre-Fill Scenario prefix expected RNG fingerprint {expected:#018x}, got {actual:#018x}"
    )]
    ScenarioRngPrestateMismatch { expected: u64, actual: u64 },
    #[error("game mode {id} is a Siege mode, which VERA20k cannot start yet")]
    UnsupportedSiegeMode { id: i32 },
    #[error(
        "pre-Fill roster requires {roster_start_count} starts but the compact launch session has {launch_participant_count} participants"
    )]
    RosterParticipantMismatch {
        roster_start_count: usize,
        launch_participant_count: usize,
    },
    #[error("map Size does not produce a valid resized pre-Fill cell rectangle")]
    InvalidMapCellExtent,
    #[error(
        "the map has room for only {found} of the {required} start positions this match needs: \
         it lists too few start waypoints, and its playable area (LocalSize) leaves no room to \
         place the rest"
    )]
    NoRoomForStarts { found: usize, required: usize },
    #[error(
        "the map's [Header] NumCoopHumanStartSpots={human_start_spots} leaves no free start \
         for all {human_count} human players"
    )]
    CoopHumanStartSpotsFull {
        human_start_spots: usize,
        human_count: usize,
    },
    #[error(
        "Quick Co-op needs a free start for each player, but the map's {start_count} start \
         positions are used up once the chosen starts are reserved"
    )]
    QuickCoopStartsExhausted { start_count: usize },
    #[error(
        "Cooperative gives each of the {house_count} players a start, observers included, \
         but the map has only {start_count} start positions; add start waypoints to the map \
         or start without observers"
    )]
    CoopStartsExhausted {
        start_count: usize,
        house_count: usize,
    },
    #[error("accepted launch Country index {index} is absent after scenario Rules reset")]
    MissingLaunchCountry { index: u16 },
}

impl PreFillScenarioPrefixPlan {
    pub fn projection(&self) -> &StockOfflinePrefixProjection {
        &self.projection
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn first_gathered_starts(&self) -> &[Waypoint] {
        &self.first_gathered_starts
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn final_gathered_starts(&self) -> &[Waypoint] {
        self.projection.final_gathered_starts()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn assignment(&self) -> &NativeStartAssignment {
        self.projection.assignment()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn first_house_timers(&self) -> &[u32] {
        &self.first_house_timers
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn second_house_timers(&self) -> &[u32] {
        &self.second_house_timers
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn rng_checkpoints(&self) -> &ScenarioPrefixRngCheckpoints {
        &self.rng_checkpoints
    }

    /// Bind the move-only E/P Rules chronology to the already-validated House,
    /// Resize, and Scenario prefix. Type/House/Cell constructors advance only
    /// the independent native-ID cursor; Scenario RNG remains byte-for-byte
    /// owned by this plan.
    pub fn bind_native_rules_receipt(
        self,
        receipt: crate::rules::process_owner::NativeScenarioRulesReceipt,
        launch: &MatchLaunchDescriptor,
        rules: &RuleSet,
    ) -> Result<BoundStockOfflineFreshPrefixPlan, PreFillScenarioPrefixPlanError> {
        launch.validate_countries(rules)?;
        let (early, rebuilt) = receipt.into_parts();
        let (early_events, first_super_weapon_type_count) = early.into_parts();
        let (rebuilt_events, final_super_weapon_type_count) = rebuilt.into_parts();
        let native_ids = crate::sim::native_identity::build_noncampaign_fresh_id_prefix(
            early_events.len(),
            first_super_weapon_type_count,
            self.first_house_timers.len(),
            rebuilt_events.len(),
            final_super_weapon_type_count,
            self.second_house_timers.len(),
            self.native_resize_width,
            self.native_resize_height,
        );
        Ok(BoundStockOfflineFreshPrefixPlan {
            scenario: self,
            native_ids,
        })
    }

    /// Validate and transfer the one pre-loading RNG prefix to the stream that
    /// later terrain Fill and Simulation construction will continue.
    pub(super) fn install_before_terrain(
        self,
        scenario_rng: &mut SimRng,
    ) -> Result<StockOfflinePrefixProjection, PreFillScenarioPrefixPlanError> {
        if scenario_rng.logical_state() != self.scenario_rng_before {
            return Err(
                PreFillScenarioPrefixPlanError::ScenarioRngPrestateMismatch {
                    expected: self.scenario_rng_before_fingerprint,
                    actual: scenario_rng.state(),
                },
            );
        }
        *scenario_rng = self.scenario_rng_after_cursor;
        debug_assert_eq!(scenario_rng.logical_state(), self.scenario_rng_after);
        Ok(self.projection)
    }
}

impl BoundStockOfflineFreshPrefixPlan {
    pub fn projection(&self) -> &StockOfflinePrefixProjection {
        self.scenario.projection()
    }

    pub(super) fn into_parts(
        self,
    ) -> (
        PreFillScenarioPrefixPlan,
        crate::sim::native_identity::NativeFreshIdPrefixReceipt,
    ) {
        (self.scenario, self.native_ids)
    }
}

impl StockOfflinePrefixProjection {
    pub fn active_scenario_waypoints(&self) -> &HashMap<u32, Waypoint> {
        &self.active_scenario_waypoints
    }

    pub fn final_gathered_starts(&self) -> &[Waypoint] {
        &self.final_gathered_starts
    }

    pub fn start_table(&self) -> &[Option<usize>] {
        &self.assignment.start_table
    }

    pub(crate) fn assignment(&self) -> &NativeStartAssignment {
        &self.assignment
    }
}

/// Prepare the complete active-stock offline prefix exactly once.
///
/// `start_waypoints` is an explicit source selected by the app boundary:
/// authored maps pass the parsed map table, while accepted Battle/FFA `.SED`
/// launches pass the provenance-bearing setup staging. Both Gather callbacks
/// operate only on the resized default-cell view derived from `[Map]`.
pub fn prepare_stock_offline_scenario_prefix_plan(
    descriptor: &MatchLaunchDescriptor,
    map_data: &MapFile,
    start_waypoints: &HashMap<u32, Waypoint>,
    launch_seed: u32,
) -> Result<PreFillScenarioPrefixPlan, PreFillScenarioPrefixPlanError> {
    let session = descriptor.session();
    let family = stock_offline_start_callback_family(session)?;
    let mut start_header = crate::map::preview::ScenarioStartHeader::default();
    start_header.read_ini(&map_data.ini);
    // A shared House is one participant.
    let launch_participant_count = (session
        .humans_with_house_leaders()
        .filter(|(index, human, leader)| index == leader && !human.is_observer())
        .count() as i32)
        .wrapping_add(session.ai_player_count)
        .max(0) as usize;
    let roster = session.pre_fill_house_roster();
    let roster_start_count = roster.required_start_count(session.ai_player_count);
    if roster_start_count != launch_participant_count {
        return Err(PreFillScenarioPrefixPlanError::RosterParticipantMismatch {
            roster_start_count,
            launch_participant_count,
        });
    }

    let mut scenario_rng = SimRng::new(u64::from(launch_seed));
    let scenario_rng_before = scenario_rng.logical_state();
    let scenario_rng_before_fingerprint = scenario_rng.state();

    let first_house_timers = advance_pre_fill_house_constructor_pass(&roster, &mut scenario_rng);
    #[cfg(any(test, feature = "test-support"))]
    let after_first_house_pass = scenario_rng.logical_state();
    let first_gathered_starts = native_gather_pre_fill_start_positions(
        start_waypoints,
        roster_start_count,
        map_data,
        &mut scenario_rng,
    )?;
    #[cfg(any(test, feature = "test-support"))]
    let after_first_gather = scenario_rng.logical_state();
    let preassignment = native_preassign_launch_start_table(session, first_gathered_starts.len());

    let final_gathered_starts = native_gather_pre_fill_start_positions(
        start_waypoints,
        roster_start_count,
        map_data,
        &mut scenario_rng,
    )?;
    // 0x0068754B..0x00687581: inside GameMode != 0, WOL Quick Co-op
    // ([0xA8B244] == 2) runs AssignStartingPoints 0x005EE9D0 in place of the
    // mode's +0x84, whatever the mode.
    let assignment = if descriptor.quick_coop_starts {
        native_assign_quick_coop_starts_from_preassignment(
            session,
            &final_gathered_starts,
            &preassignment,
            &mut scenario_rng,
        )?
    } else {
        match family {
            StockOfflineStartCallbackFamily::Battle => {
                native_assign_launch_starts_from_preassignment(
                    session,
                    &final_gathered_starts,
                    &preassignment,
                    &mut scenario_rng,
                )
            }
            StockOfflineStartCallbackFamily::Cooperative => {
                let human_start_spots = start_header.coop_human_start_spots().max(0) as usize;
                native_assign_cooperative_starts_from_preassignment(
                    session,
                    &final_gathered_starts,
                    &preassignment,
                    human_start_spots,
                    roster.nonobserver_human_count(),
                    &mut scenario_rng,
                )?
            }
        }
    };
    #[cfg(any(test, feature = "test-support"))]
    let after_second_gather_and_chooser = scenario_rng.logical_state();

    // Native deletes every disposable first-pass House here. Destruction and
    // the following rules/basic reset consume no Scenario draw, so the second
    // pass begins at the chooser's exact cursor.
    #[cfg(any(test, feature = "test-support"))]
    let after_zero_draw_reset = scenario_rng.logical_state();
    let second_house_timers = advance_pre_fill_house_constructor_pass(&roster, &mut scenario_rng);
    let scenario_rng_after = scenario_rng.logical_state();
    #[cfg(any(test, feature = "test-support"))]
    let after_second_house_pass = scenario_rng_after.clone();

    Ok(PreFillScenarioPrefixPlan {
        start_header,
        projection: StockOfflinePrefixProjection {
            active_scenario_waypoints: start_waypoints.clone(),
            final_gathered_starts,
            assignment,
        },
        #[cfg(any(test, feature = "test-support"))]
        first_gathered_starts,
        first_house_timers,
        second_house_timers,
        scenario_rng_before,
        scenario_rng_before_fingerprint,
        scenario_rng_after,
        scenario_rng_after_cursor: scenario_rng,
        native_resize_width: map_data.header.width,
        native_resize_height: map_data.header.height,
        #[cfg(any(test, feature = "test-support"))]
        rng_checkpoints: ScenarioPrefixRngCheckpoints {
            after_first_house_pass,
            after_first_gather,
            after_second_gather_and_chooser,
            after_zero_draw_reset,
            after_second_house_pass,
        },
    })
}

fn advance_pre_fill_house_constructor_pass(
    roster: &PreFillHouseRoster,
    rng: &mut SimRng,
) -> Vec<u32> {
    let mut timers = Vec::with_capacity(roster.created_house_count());
    for _human in roster.human_nodes() {
        timers.push(draw_house_constructor_timer(rng));
    }
    for _slot in roster.ai_slots().iter().filter(|slot| slot.valid) {
        timers.push(draw_house_constructor_timer(rng));
    }
    for _fixed in roster.fixed_tail() {
        timers.push(draw_house_constructor_timer(rng));
    }
    timers
}

pub(super) fn draw_house_constructor_timer(rng: &mut SimRng) -> u32 {
    rng.next_range_u32_inclusive(HOUSE_CONSTRUCTOR_TIMER_MIN, HOUSE_CONSTRUCTOR_TIMER_MAX)
}

/// The start callbacks belong to the mode's class, not to its row:
/// `MPGameOptions__LoadAllGameModes @ 0x005D7CE0` builds every row of a
/// section with that section's class, so a mod's added or changed row of
/// `[Battle]` starts as Battle does. Vtable slot +0x84 is `0x005D6C70` for
/// Battle, ManBattle, Unholy and FreeForAll and `0x005C2EF0` for Cooperative.
///
/// RESIDUAL: Siege's `0x005CA800` (the defender takes the first gathered
/// start) is not ported. Retail has no `[Siege]` row; a mod's is refused.
pub fn stock_offline_start_callback_family(
    session: &SkirmishLaunchSession,
) -> Result<StockOfflineStartCallbackFamily, PreFillScenarioPrefixPlanError> {
    use crate::skirmish_modes::MpModeClass;
    match session.mode.class {
        MpModeClass::Cooperative => Ok(StockOfflineStartCallbackFamily::Cooperative),
        MpModeClass::Siege => Err(PreFillScenarioPrefixPlanError::UnsupportedSiegeMode {
            id: session.mode.id,
        }),
        _ => Ok(StockOfflineStartCallbackFamily::Battle),
    }
}
