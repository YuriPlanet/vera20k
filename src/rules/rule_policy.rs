//! Rule choices admitted for scenario construction and live simulation.

/// Selects the native rule path used when building a scenario.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum BootstrapRuleProfile {
    #[default]
    Campaign,
    Multiplayer,
}

impl BootstrapRuleProfile {
    pub const fn uses_multiplayer_rules(self) -> bool {
        matches!(self, Self::Multiplayer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum AllowWinTagsPolicy {
    RequireClear,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ComputerParanoidPolicy {
    Disabled,
    React,
}

/// Admission of map-authored crate overlays at load time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum AuthoredCratePolicy {
    Keep,
    Skip,
}

/// Whether the AI may replace unavailable or invalidated base-plan nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum AiBasePolicy {
    RetainAuthoredNodes,
    ReplaceUnavailableNodes,
}

/// How factory selection interprets `DoubleOwned=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DoubleOwnedPolicy {
    OwnerMask,
    AllCountries,
}

/// The set of live TeamClass instances that count toward a TeamType's `Max=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum TeamLimitScope {
    AllHouses,
    PerHouse,
}

/// Independently selectable rule domains. Stock admission chooses
/// a preset from the mode and mode class. Live sessions retain the admitted
/// value. Offline Skirmish (GameMode 5) uses multiplayer rules with local
/// execution. Binary choices share one flag word; named selectors describe
/// the behavior at subsystem boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct RulePolicy {
    flags: u64,
}

const _: () = assert!(std::mem::size_of::<RulePolicy>() <= 16);

impl Default for RulePolicy {
    fn default() -> Self {
        Self::stock_campaign()
    }
}

impl RulePolicy {
    const MULTIPLAYER_BOOTSTRAP: u64 = 1 << 0;
    const IGNORE_ALLOW_WIN_TAGS: u64 = 1 << 1;
    const PARANOID_REACTIONS: u64 = 1 << 2;
    const SKIP_AUTHORED_CRATES: u64 = 1 << 3;
    const REPLACE_UNAVAILABLE_BASE_NODES: u64 = 1 << 4;
    const DOUBLE_OWNED_ALL_COUNTRIES: u64 = 1 << 5;
    const MOD_MAP_ADDITIONS: u64 = 1 << 6;
    const TEAM_LIMIT_PER_HOUSE: u64 = 1 << 7;
    const DISSOLVE_UNFILLED_TEAMS: u64 = 1 << 8;
    const SHARED_ALLIED_VICTORY: u64 = 1 << 9;
    const SKIP_NETWORK_SCORE_DIALOG: u64 = 1 << 10;
    const PARANOID_IGNORES_ALLIES_ALLOWED: u64 = 1 << 11;
    const NETWORK_DIPLOMACY_BRIEFING: u64 = 1 << 12;
    const STARTING_HOUSE_ROLE_ALLIANCES: u64 = 1 << 13;
    const ARES_PSYCHIC_REVEAL_EXCLUSIONS: u64 = 1 << 14;
    const ALLOWS_CONSTRUCTION_YARD_REDEPLOY: u64 = 1 << 15;
    const ALLOWS_IDLE_OBJECT_SLEEP: u64 = 1 << 16;
    const ALLOWS_OWNERLESS_TERRAIN_LOGIC: u64 = 1 << 17;
    const COUNTS_PLAYER_CONTROL_AS_HUMAN: u64 = 1 << 18;
    const USES_COUNTRY_DIFFICULTY_BIASES: u64 = 1 << 19;
    const SKIPS_LOCAL_PLAYER_AUTHORED_BUILDINGS: u64 = 1 << 20;
    const USES_MANAGED_AI_BASE: u64 = 1 << 21;
    const USES_AI_ECONOMY: u64 = 1 << 22;
    const RUNS_AI_STRATEGY: u64 = 1 << 23;
    const USES_HOUSE_TEAM_RULES: u64 = 1 << 24;
    const USES_CAMPAIGN_DISCOVERY: u64 = 1 << 25;
    const USES_CAMPAIGN_BASE_DEFENSE: u64 = 1 << 26;
    const USES_DIRECTED_HUNTER_SEEKER_TARGETING: u64 = 1 << 27;
    const USES_MATCH_CRATES: u64 = 1 << 28;
    const USES_ELIMINATION_RESULTS: u64 = 1 << 29;
    const USES_ROOM_OPTIONS: u64 = 1 << 30;
    const USES_CAMPAIGN_DIPLOMACY: u64 = 1 << 31;
    const ALLOWS_MANUAL_ALLIANCES: u64 = 1 << 32;

    pub const fn stock_campaign() -> Self {
        Self { flags: 0 }
            .with_allows_ownerless_terrain_logic(true)
            .with_counts_player_control_as_human(true)
            .with_uses_campaign_discovery(true)
            .with_uses_campaign_base_defense(true)
            .with_uses_campaign_diplomacy(true)
    }

    pub const fn stock_multiplayer() -> Self {
        Self::stock_campaign()
            .with_bootstrap(BootstrapRuleProfile::Multiplayer)
            .with_allow_win_tags(AllowWinTagsPolicy::Ignore)
            .with_computer_paranoid(ComputerParanoidPolicy::React)
            .with_crates(AuthoredCratePolicy::Skip)
            .with_ai_base(AiBasePolicy::ReplaceUnavailableNodes)
            .with_double_owned(DoubleOwnedPolicy::AllCountries)
            .with_mod_map_additions(true)
            .with_team_creation_limit_scope(TeamLimitScope::PerHouse)
            .with_dissolve_unfilled_teams(true)
            .with_ares_psychic_reveal_exclusions(true)
            .with_allows_construction_yard_redeploy(true)
            .with_allows_idle_object_sleep(true)
            .with_counts_player_control_as_human(false)
            .with_uses_country_difficulty_biases(true)
            .with_skips_local_player_authored_buildings(true)
            .with_uses_managed_ai_base(true)
            .with_uses_ai_economy(true)
            .with_runs_ai_strategy(true)
            .with_uses_house_team_rules(true)
            .with_uses_campaign_discovery(false)
            .with_uses_campaign_base_defense(false)
            .with_uses_directed_hunter_seeker_targeting(true)
            .with_uses_match_crates(true)
            .with_uses_elimination_results(true)
            .with_uses_room_options(true)
            .with_uses_campaign_diplomacy(false)
            .with_allows_manual_alliances(true)
    }

    const fn has_flag(self, flag: u64) -> bool {
        self.flags & flag != 0
    }

    const fn with_flag(mut self, flag: u64, enabled: bool) -> Self {
        if enabled {
            self.flags |= flag;
        } else {
            self.flags &= !flag;
        }
        self
    }

    /// Admit eligible player yard pack-up orders (0x0044A8DF, 0x0044F602, 0x0044375E).
    pub const fn allows_construction_yard_redeploy(self) -> bool {
        self.has_flag(Self::ALLOWS_CONSTRUCTION_YARD_REDEPLOY)
    }

    pub const fn with_allows_construction_yard_redeploy(self, enabled: bool) -> Self {
        self.with_flag(Self::ALLOWS_CONSTRUCTION_YARD_REDEPLOY, enabled)
    }

    /// Use VERA's idle-object sleep under its trigger and repair guards.
    pub const fn allows_idle_object_sleep(self) -> bool {
        self.has_flag(Self::ALLOWS_IDLE_OBJECT_SLEEP)
    }

    pub const fn with_allows_idle_object_sleep(self, enabled: bool) -> Self {
        self.with_flag(Self::ALLOWS_IDLE_OBJECT_SLEEP, enabled)
    }

    /// Admit ownerless terrain to logic without the identity test (0x005F501B).
    pub const fn allows_ownerless_terrain_logic(self) -> bool {
        self.has_flag(Self::ALLOWS_OWNERLESS_TERRAIN_LOGIC)
    }

    pub const fn with_allows_ownerless_terrain_logic(self, enabled: bool) -> Self {
        self.with_flag(Self::ALLOWS_OWNERLESS_TERRAIN_LOGIC, enabled)
    }

    /// HouseClass::IsControlledByHuman also honors PlayerControl (0x0050B730).
    pub const fn counts_player_control_as_human(self) -> bool {
        self.has_flag(Self::COUNTS_PLAYER_CONTROL_AS_HUMAN)
    }

    pub const fn with_counts_player_control_as_human(self, enabled: bool) -> Self {
        self.with_flag(Self::COUNTS_PLAYER_CONTROL_AS_HUMAN, enabled)
    }

    /// Multiply House difficulty biases by the selected country's values (0x004F6EC0).
    pub const fn uses_country_difficulty_biases(self) -> bool {
        self.has_flag(Self::USES_COUNTRY_DIFFICULTY_BIASES)
    }

    pub const fn with_uses_country_difficulty_biases(self, enabled: bool) -> Self {
        self.with_flag(Self::USES_COUNTRY_DIFFICULTY_BIASES, enabled)
    }

    /// Skip the local player's authored structures in local multiplayer setup (0x0044F8CB).
    pub const fn skips_local_player_authored_buildings(self) -> bool {
        self.has_flag(Self::SKIPS_LOCAL_PLAYER_AUTHORED_BUILDINGS)
    }

    pub const fn with_skips_local_player_authored_buildings(self, enabled: bool) -> Self {
        self.with_flag(Self::SKIPS_LOCAL_PLAYER_AUTHORED_BUILDINGS, enabled)
    }

    /// Use the AI's automatic base construction and deployment cycle.
    pub const fn uses_managed_ai_base(self) -> bool {
        self.has_flag(Self::USES_MANAGED_AI_BASE)
    }

    pub const fn with_uses_managed_ai_base(self, enabled: bool) -> Self {
        self.with_flag(Self::USES_MANAGED_AI_BASE, enabled)
    }

    /// Use the AI's multiplayer sale and ore-payment rules.
    pub const fn uses_ai_economy(self) -> bool {
        self.has_flag(Self::USES_AI_ECONOMY)
    }

    pub const fn with_uses_ai_economy(self, enabled: bool) -> Self {
        self.with_flag(Self::USES_AI_ECONOMY, enabled)
    }

    /// Run the AI's periodic enemy search and autonomous strategy decisions.
    pub const fn runs_ai_strategy(self) -> bool {
        self.has_flag(Self::RUNS_AI_STRATEGY)
    }

    pub const fn with_runs_ai_strategy(self, enabled: bool) -> Self {
        self.with_flag(Self::RUNS_AI_STRATEGY, enabled)
    }

    /// Load and run AI team definitions under their owning House's rules.
    pub const fn uses_house_team_rules(self) -> bool {
        self.has_flag(Self::USES_HOUSE_TEAM_RULES)
    }

    pub const fn with_uses_house_team_rules(self, enabled: bool) -> Self {
        self.with_flag(Self::USES_HOUSE_TEAM_RULES, enabled)
    }

    /// Use campaign discovery rules for sight, services and targeting.
    pub const fn uses_campaign_discovery(self) -> bool {
        self.has_flag(Self::USES_CAMPAIGN_DISCOVERY)
    }

    pub const fn with_uses_campaign_discovery(self, enabled: bool) -> Self {
        self.with_flag(Self::USES_CAMPAIGN_DISCOVERY, enabled)
    }

    /// Use the campaign response rules for armed victims and eligible defenders.
    pub const fn uses_campaign_base_defense(self) -> bool {
        self.has_flag(Self::USES_CAMPAIGN_BASE_DEFENSE)
    }

    pub const fn with_uses_campaign_base_defense(self, enabled: bool) -> Self {
        self.with_flag(Self::USES_CAMPAIGN_BASE_DEFENSE, enabled)
    }

    /// Direct Ares Hunter Seekers by the controlling House's role and enemy.
    pub const fn uses_directed_hunter_seeker_targeting(self) -> bool {
        self.has_flag(Self::USES_DIRECTED_HUNTER_SEEKER_TARGETING)
    }

    pub const fn with_uses_directed_hunter_seeker_targeting(self, enabled: bool) -> Self {
        self.with_flag(Self::USES_DIRECTED_HUNTER_SEEKER_TARGETING, enabled)
    }

    /// Use the match crate lifecycle, including timed slots and validated payloads.
    pub const fn uses_match_crates(self) -> bool {
        self.has_flag(Self::USES_MATCH_CRATES)
    }

    pub const fn with_uses_match_crates(self, enabled: bool) -> Self {
        self.with_flag(Self::USES_MATCH_CRATES, enabled)
    }

    /// Resolve match outcomes through House elimination and its cleanup rules.
    pub const fn uses_elimination_results(self) -> bool {
        self.has_flag(Self::USES_ELIMINATION_RESULTS)
    }

    pub const fn with_uses_elimination_results(self, enabled: bool) -> Self {
        self.with_flag(Self::USES_ELIMINATION_RESULTS, enabled)
    }

    /// Use room options and the native pregame flags in scenario setup.
    pub const fn uses_room_options(self) -> bool {
        self.has_flag(Self::USES_ROOM_OPTIONS)
    }

    pub const fn with_uses_room_options(self, enabled: bool) -> Self {
        self.with_flag(Self::USES_ROOM_OPTIONS, enabled)
    }

    /// Show the mission briefing instead of the diplomacy panel in campaign UI.
    pub const fn uses_campaign_diplomacy(self) -> bool {
        self.has_flag(Self::USES_CAMPAIGN_DIPLOMACY)
    }

    pub const fn with_uses_campaign_diplomacy(self, enabled: bool) -> Self {
        self.with_flag(Self::USES_CAMPAIGN_DIPLOMACY, enabled)
    }

    /// Admit the manual Ally order (0x00731840) subject to the separate alliance rules.
    pub const fn allows_manual_alliances(self) -> bool {
        self.has_flag(Self::ALLOWS_MANUAL_ALLIANCES)
    }

    pub const fn with_allows_manual_alliances(self, enabled: bool) -> Self {
        self.with_flag(Self::ALLOWS_MANUAL_ALLIANCES, enabled)
    }

    pub const fn bootstrap(self) -> BootstrapRuleProfile {
        if self.has_flag(Self::MULTIPLAYER_BOOTSTRAP) {
            BootstrapRuleProfile::Multiplayer
        } else {
            BootstrapRuleProfile::Campaign
        }
    }

    pub const fn with_bootstrap(self, profile: BootstrapRuleProfile) -> Self {
        self.with_flag(
            Self::MULTIPLAYER_BOOTSTRAP,
            profile.uses_multiplayer_rules(),
        )
    }

    pub const fn allow_win_tags(self) -> AllowWinTagsPolicy {
        if self.has_flag(Self::IGNORE_ALLOW_WIN_TAGS) {
            AllowWinTagsPolicy::Ignore
        } else {
            AllowWinTagsPolicy::RequireClear
        }
    }

    pub const fn with_allow_win_tags(self, policy: AllowWinTagsPolicy) -> Self {
        self.with_flag(
            Self::IGNORE_ALLOW_WIN_TAGS,
            matches!(policy, AllowWinTagsPolicy::Ignore),
        )
    }

    pub const fn computer_paranoid(self) -> ComputerParanoidPolicy {
        if self.has_flag(Self::PARANOID_REACTIONS) {
            ComputerParanoidPolicy::React
        } else {
            ComputerParanoidPolicy::Disabled
        }
    }

    pub const fn with_computer_paranoid(self, policy: ComputerParanoidPolicy) -> Self {
        self.with_flag(
            Self::PARANOID_REACTIONS,
            matches!(policy, ComputerParanoidPolicy::React),
        )
    }

    pub const fn crates(self) -> AuthoredCratePolicy {
        if self.has_flag(Self::SKIP_AUTHORED_CRATES) {
            AuthoredCratePolicy::Skip
        } else {
            AuthoredCratePolicy::Keep
        }
    }

    pub const fn with_crates(self, policy: AuthoredCratePolicy) -> Self {
        self.with_flag(
            Self::SKIP_AUTHORED_CRATES,
            matches!(policy, AuthoredCratePolicy::Skip),
        )
    }

    pub const fn ai_base(self) -> AiBasePolicy {
        if self.has_flag(Self::REPLACE_UNAVAILABLE_BASE_NODES) {
            AiBasePolicy::ReplaceUnavailableNodes
        } else {
            AiBasePolicy::RetainAuthoredNodes
        }
    }

    pub const fn with_ai_base(self, policy: AiBasePolicy) -> Self {
        self.with_flag(
            Self::REPLACE_UNAVAILABLE_BASE_NODES,
            matches!(policy, AiBasePolicy::ReplaceUnavailableNodes),
        )
    }

    pub const fn double_owned(self) -> DoubleOwnedPolicy {
        if self.has_flag(Self::DOUBLE_OWNED_ALL_COUNTRIES) {
            DoubleOwnedPolicy::AllCountries
        } else {
            DoubleOwnedPolicy::OwnerMask
        }
    }

    pub const fn with_double_owned(self, policy: DoubleOwnedPolicy) -> Self {
        self.with_flag(
            Self::DOUBLE_OWNED_ALL_COUNTRIES,
            matches!(policy, DoubleOwnedPolicy::AllCountries),
        )
    }

    pub const fn mod_map_additions(self) -> bool {
        self.has_flag(Self::MOD_MAP_ADDITIONS)
    }

    pub const fn with_mod_map_additions(self, enabled: bool) -> Self {
        self.with_flag(Self::MOD_MAP_ADDITIONS, enabled)
    }

    pub const fn shared_allied_victory(self) -> bool {
        self.has_flag(Self::SHARED_ALLIED_VICTORY)
    }

    pub const fn with_shared_allied_victory(self, enabled: bool) -> Self {
        self.with_flag(Self::SHARED_ALLIED_VICTORY, enabled)
    }

    pub const fn skips_network_score_dialog(self) -> bool {
        self.has_flag(Self::SKIP_NETWORK_SCORE_DIALOG)
    }

    pub const fn with_skips_network_score_dialog(self, enabled: bool) -> Self {
        self.with_flag(Self::SKIP_NETWORK_SCORE_DIALOG, enabled)
    }

    /// `Computer_Paranoid @ 0x00501640` can pass the AlliesAllowed gate
    /// when the stock mode's virtual at +4 returns true (0x00501659).
    pub(crate) const fn paranoid_ignores_allies_allowed(self) -> bool {
        self.has_flag(Self::PARANOID_IGNORES_ALLIES_ALLOWED)
    }

    pub const fn with_paranoid_ignores_allies_allowed(self, enabled: bool) -> Self {
        self.with_flag(Self::PARANOID_IGNORES_ALLIES_ALLOWED, enabled)
    }

    /// The LAN/WOL sidebar request chooses the briefing when both mode
    /// virtuals answer true (0x00653918..0x00653938).
    pub const fn restates_mission_for_network_diplomacy(self) -> bool {
        self.has_flag(Self::NETWORK_DIPLOMACY_BRIEFING)
    }

    pub const fn with_network_diplomacy_briefing(self, enabled: bool) -> Self {
        self.with_flag(Self::NETWORK_DIPLOMACY_BRIEFING, enabled)
    }

    /// Add mutual alliances among starting Houses with matching human roles.
    pub(crate) const fn allies_starting_houses_by_role(self) -> bool {
        self.has_flag(Self::STARTING_HOUSE_ROLE_ALLIANCES)
    }

    pub const fn with_starting_house_role_alliances(self, enabled: bool) -> Self {
        self.with_flag(Self::STARTING_HOUSE_ROLE_ALLIANCES, enabled)
    }

    /// Ares RevealHelper (0x100692D5..0x10069315 and 0x10069480)
    /// excludes three map-relative cells in competitive LAN/WOL games.
    pub(crate) const fn ares_psychic_reveal_exclusions(self) -> bool {
        self.has_flag(Self::ARES_PSYCHIC_REVEAL_EXCLUSIONS)
    }

    pub const fn with_ares_psychic_reveal_exclusions(self, enabled: bool) -> Self {
        self.with_flag(Self::ARES_PSYCHIC_REVEAL_EXCLUSIONS, enabled)
    }

    /// `HouseClass::Update @ 0x004F8440` checks the AllowWin count only
    /// in the mode-zero victory arm (`0x004F85F7..0x004F8605`).
    pub(crate) const fn victory_requires_cleared_allow_win_tags(self) -> bool {
        matches!(self.allow_win_tags(), AllowWinTagsPolicy::RequireClear)
    }

    /// Admission of diplomacy and base requests into `Computer_Paranoid @
    /// 0x00501640`. Each caller applies its own House and Rules checks.
    pub(crate) const fn permits_paranoid_reactions(self) -> bool {
        matches!(self.computer_paranoid(), ComputerParanoidPolicy::React)
    }

    /// `TeamTypeClass::Create_Team @ 0x006F09C0` checks mode at 0x006F0A16.
    pub(crate) const fn team_creation_limit_scope(self) -> TeamLimitScope {
        if self.has_flag(Self::TEAM_LIMIT_PER_HOUSE) {
            TeamLimitScope::PerHouse
        } else {
            TeamLimitScope::AllHouses
        }
    }

    pub(crate) const fn with_team_creation_limit_scope(self, scope: TeamLimitScope) -> Self {
        self.with_flag(
            Self::TEAM_LIMIT_PER_HOUSE,
            matches!(scope, TeamLimitScope::PerHouse),
        )
    }

    pub const fn dissolves_unfilled_teams(self) -> bool {
        self.has_flag(Self::DISSOLVE_UNFILLED_TEAMS)
    }

    pub const fn with_dissolve_unfilled_teams(self, enabled: bool) -> Self {
        self.with_flag(Self::DISSOLVE_UNFILLED_TEAMS, enabled)
    }

    /// `TeamClass::AI @ 0x006E9140` uses `DissolveUnfilledTeamDelay=` for
    /// never-full empty teams in the nonzero mode arm at `0x006E92AD`.
    pub(crate) const fn unfilled_team_timeout(self, delay: i32) -> Option<i32> {
        if self.dissolves_unfilled_teams() {
            Some(delay)
        } else {
            None
        }
    }
}
