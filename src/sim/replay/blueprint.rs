//! Authored frame-zero launch inputs. Live transport and shell settings never
//! cross this boundary. The admitted session owns rules and source authority.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::ReplayStartOptions;
use crate::rules::personal_rules::PersonalRules;
use crate::rules::rule_policy::BootstrapRuleProfile;
use crate::sim::campaign_carry_over::CampaignCarryOver;
use crate::sim::game_options::GameOptions;
use crate::sim::scenario_bootstrap::{
    AuthoredCampaignStart, MatchLaunchDescriptor, MatchNetworkStart,
};
use crate::sim::scenario_session::ScenarioSession;
use crate::skirmish_launch::{
    PreFillHouseRoster, SKIRMISH_MAX_SLOT_COUNT, SkirmishAiSlot, SkirmishHumanSlot,
    SkirmishLaunchMode, SkirmishLaunchOptions, SkirmishLaunchSession,
};
use crate::skirmish_modes::MpModeClass;

/// Versioned by its containing `ReplayStartup`. The admitted session stays
/// outside the tagged payload so JSON decodes its integer-keyed maps directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchStartBlueprint {
    admitted: ScenarioSession,
    #[serde(flatten)]
    launch: ReplayLaunch,
}

/// Campaign identifies an authored scripted lockstep match with campaign rules.
/// The `generated` tag is reserved for an accepted random-map startup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReplayLaunch {
    Skirmish(AuthoredMatchStart),
    Cooperative(AuthoredMatchStart),
    Campaign(CampaignReplayStart),
}

/// Old scripted slot launches retain their original Campaign payload.
/// Authored House launches carry the role roster that built their world.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CampaignReplayStart {
    MapHouses(MapHouseReplayStart),
    Slots(AuthoredMatchStart),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapHouseReplayStart {
    map_houses: AuthoredCampaignStart,
}

#[derive(Clone, Copy)]
pub enum ReplayLaunchInput<'a> {
    Slots(&'a SkirmishLaunchSession),
    MapHouses(&'a AuthoredCampaignStart),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoredMatchStart {
    mode: MatchMode,
    humans: Vec<SkirmishHumanSlot>,
    local_player_index: usize,
    opponents: Vec<SkirmishAiSlot>,
    constructor_roster: PreFillHouseRoster,
    options: GameOptions,
    teams: Option<u8>,
    quick_coop_starts: bool,
    personal_rules: PersonalRules,
    #[serde(skip_serializing_if = "Option::is_none")]
    campaign_carry_over: Option<CampaignCarryOver>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MatchMode {
    class: MpModeClass,
    id: i32,
    override_file: String,
    allies_allowed: bool,
    must_ally: bool,
}

impl MatchStartBlueprint {
    pub(super) fn capture(
        launch: ReplayLaunchInput<'_>,
        admitted: &ScenarioSession,
        options: ReplayStartOptions,
    ) -> Result<Self> {
        let launch = match launch {
            ReplayLaunchInput::Slots(launch) => launch,
            ReplayLaunchInput::MapHouses(start) => {
                anyhow::ensure!(
                    !options.quick_coop_starts
                        && options.campaign_carry_over.is_none()
                        && !options.personal_rules.in_effect(),
                    "MapHouses playback does not use stock slot launch options"
                );
                let blueprint = Self {
                    admitted: admitted.clone().with_replay_view(None),
                    launch: ReplayLaunch::Campaign(CampaignReplayStart::MapHouses(
                        MapHouseReplayStart {
                            map_houses: start.clone(),
                        },
                    )),
                };
                blueprint.validate()?;
                return Ok(blueprint);
            }
        };
        let mode = &launch.mode;
        let facts = AuthoredMatchStart {
            mode: MatchMode {
                class: mode.class,
                id: mode.id,
                override_file: mode.override_file.clone(),
                allies_allowed: mode.allies_allowed,
                must_ally: mode.must_ally,
            },
            humans: launch.humans.clone(),
            local_player_index: launch.local_player_index,
            opponents: launch.opponents.clone(),
            constructor_roster: launch.pre_fill_house_roster().into_owned(),
            options: launch.options.to_game_options(launch.ai_player_count),
            teams: launch.options.teams,
            quick_coop_starts: options.quick_coop_starts,
            personal_rules: options.personal_rules,
            campaign_carry_over: options.campaign_carry_over,
        };
        let launch = if scripted_lockstep(admitted) {
            ReplayLaunch::Campaign(CampaignReplayStart::Slots(facts))
        } else if mode.class == MpModeClass::Cooperative {
            ReplayLaunch::Cooperative(facts)
        } else {
            ReplayLaunch::Skirmish(facts)
        };
        let blueprint = Self {
            admitted: admitted.clone(),
            launch,
        };
        blueprint.validate()?;
        Ok(blueprint)
    }

    fn slot_facts(&self) -> Option<&AuthoredMatchStart> {
        match self.launch() {
            ReplayLaunch::Skirmish(facts)
            | ReplayLaunch::Cooperative(facts)
            | ReplayLaunch::Campaign(CampaignReplayStart::Slots(facts)) => Some(facts),
            ReplayLaunch::Campaign(CampaignReplayStart::MapHouses(_)) => None,
        }
    }

    pub(super) fn map_houses(&self) -> Option<&AuthoredCampaignStart> {
        match self.launch() {
            ReplayLaunch::Campaign(CampaignReplayStart::MapHouses(start)) => {
                Some(&start.map_houses)
            }
            ReplayLaunch::Skirmish(_)
            | ReplayLaunch::Cooperative(_)
            | ReplayLaunch::Campaign(CampaignReplayStart::Slots(_)) => None,
        }
    }

    pub fn launch(&self) -> &ReplayLaunch {
        &self.launch
    }

    pub(super) fn admitted(&self) -> &ScenarioSession {
        &self.admitted
    }

    pub(super) fn campaign_carry_over(&self) -> Option<&CampaignCarryOver> {
        self.slot_facts()?.campaign_carry_over.as_ref()
    }

    pub(super) fn validate(&self) -> Result<()> {
        if let Some(start) = self.map_houses() {
            start.validate()?;
            anyhow::ensure!(
                self.admitted.execution_topology().is_lockstep()
                    && self.admitted.multiplayer_mode_class().is_none()
                    && self.admitted.campaign_mission_counter() == Some(start.mission_counter)
                    && self.admitted.rule_policy() == start.rule_policy(),
                "MapHouses replay disagrees with admitted campaign startup"
            );
            return Ok(());
        }
        let facts = self
            .slot_facts()
            .context("replay launch has no slot startup")?;
        let scripted = scripted_lockstep(&self.admitted);
        anyhow::ensure!(
            match self.launch() {
                ReplayLaunch::Skirmish(_) =>
                    facts.mode.class != MpModeClass::Cooperative && !scripted,
                ReplayLaunch::Cooperative(_) =>
                    facts.mode.class == MpModeClass::Cooperative && !scripted,
                ReplayLaunch::Campaign(CampaignReplayStart::Slots(_)) => scripted,
                ReplayLaunch::Campaign(CampaignReplayStart::MapHouses(_)) => false,
            } && self.admitted.multiplayer_mode_class() == Some(facts.mode.class),
            "replay launch kind disagrees with its admitted mode"
        );
        anyhow::ensure!(
            facts.campaign_carry_over.is_none()
                || matches!(
                    self.launch(),
                    ReplayLaunch::Cooperative(_) | ReplayLaunch::Campaign(_)
                ) && self.admitted.execution_topology().is_lockstep(),
            "only a Cooperative or scripted lockstep replay can inherit a campaign record"
        );
        anyhow::ensure!(
            facts.local_player_index < facts.humans.len()
                && facts.humans.len() + facts.opponents.len() <= SKIRMISH_MAX_SLOT_COUNT
                && facts.options.ai_players >= 0
                && facts.options.ai_players as usize <= SKIRMISH_MAX_SLOT_COUNT,
            "invalid replay launch seats"
        );
        anyhow::ensure!(
            !facts
                .humans
                .iter()
                .any(|human| human.slot.country_random || human.slot.color_random)
                && !facts.opponents.iter().any(|opponent| {
                    opponent.country_random
                        || opponent.color_random
                        || usize::from(opponent.raw_slot_index) >= SKIRMISH_MAX_SLOT_COUNT
                })
                && facts
                    .opponents
                    .windows(2)
                    .all(|pair| { pair[0].raw_slot_index < pair[1].raw_slot_index }),
            "invalid replay launch choices"
        );
        Ok(())
    }

    pub(super) fn launch_descriptor(&self, map_file: &str) -> Result<MatchLaunchDescriptor> {
        self.validate()?;
        let facts = self
            .slot_facts()
            .context("replay uses the authored House roster rather than player slots")?;
        let mode = SkirmishLaunchMode {
            class: facts.mode.class,
            id: facts.mode.id,
            override_file: facts.mode.override_file.clone(),
            allies_allowed: facts.mode.allies_allowed,
            must_ally: facts.mode.must_ally,
            ui_name_key: String::new(),
            tooltip_key: String::new(),
            map_filter: String::new(),
            random_maps_allowed: false,
        };
        let mut options = SkirmishLaunchOptions::from_game_options(&facts.options);
        options.teams = facts.teams;
        let mut session = SkirmishLaunchSession::from_rows(
            mode,
            Some(map_file.to_owned()),
            facts.humans.clone(),
            facts.local_player_index,
            facts.opponents.clone(),
            options,
        );
        session.ai_player_count = facts.options.ai_players;
        session.retain_recorded_house_roster(facts.constructor_roster.clone());
        let mut launch = MatchLaunchDescriptor::from_resolved(session)
            .map_err(anyhow::Error::msg)
            .context("invalid replay launch")?
            .with_control_grants(self.admitted.control_grants().to_vec());
        if !self.admitted.has_stock_rule_policy() {
            launch = launch.with_rule_policy(self.admitted.rule_policy());
        }
        if facts.quick_coop_starts {
            launch = launch.with_quick_coop_starts();
        }
        launch.set_personal_rules(facts.personal_rules.clone());
        if self.admitted.execution_topology().is_lockstep() {
            launch.set_network_start(MatchNetworkStart {
                max_ahead_frames: self.admitted.network_max_ahead_frames(),
                tournament: self.admitted.wol_tournament(),
                squad_lists: self.admitted.wol_squad_lists().clone(),
            });
        }
        Ok(launch)
    }
}

fn scripted_lockstep(admitted: &ScenarioSession) -> bool {
    admitted.execution_topology().is_lockstep()
        && admitted.rule_policy().bootstrap() == BootstrapRuleProfile::Campaign
}
