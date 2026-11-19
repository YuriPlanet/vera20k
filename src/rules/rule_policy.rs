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

    pub(crate) const fn forces_listed_ai_trigger_enables(self) -> bool {
        self.uses_multiplayer_rules()
    }

    pub(crate) const fn uses_default_task_force_member_tech_level(self) -> bool {
        self.uses_multiplayer_rules()
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

/// Independently selectable rules. Stock admission and legacy decoding choose
/// a preset from the mode and mode class. Live sessions retain the admitted
/// value. Offline Skirmish (GameMode 5) uses multiplayer rules with local
/// execution. Binary choices share one flag word; named selectors describe
/// the behavior at subsystem boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct RulePolicy {
    flags: u32,
}

const _: () = assert!(std::mem::size_of::<RulePolicy>() <= 16);

impl Default for RulePolicy {
    fn default() -> Self {
        Self::stock_campaign()
    }
}

impl RulePolicy {
    const MULTIPLAYER_BOOTSTRAP: u32 = 1 << 0;
    const IGNORE_ALLOW_WIN_TAGS: u32 = 1 << 1;
    const PARANOID_REACTIONS: u32 = 1 << 2;
    const SKIP_AUTHORED_CRATES: u32 = 1 << 3;
    const REPLACE_UNAVAILABLE_BASE_NODES: u32 = 1 << 4;
    const DOUBLE_OWNED_ALL_COUNTRIES: u32 = 1 << 5;
    const MOD_MAP_ADDITIONS: u32 = 1 << 6;
    const TEAM_LIMIT_PER_HOUSE: u32 = 1 << 7;
    const DISSOLVE_UNFILLED_TEAMS: u32 = 1 << 8;
    const SHARED_ALLIED_VICTORY: u32 = 1 << 9;
    const SKIP_NETWORK_SCORE_DIALOG: u32 = 1 << 10;
    const PARANOID_IGNORES_ALLIES_ALLOWED: u32 = 1 << 11;
    const NETWORK_DIPLOMACY_BRIEFING: u32 = 1 << 12;
    const STARTING_HOUSE_ROLE_ALLIANCES: u32 = 1 << 13;
    const ARES_PSYCHIC_REVEAL_EXCLUSIONS: u32 = 1 << 14;
    const AI_MCV_DEPLOY_HUNT: u32 = 1 << 15;

    pub const fn stock_campaign() -> Self {
        Self { flags: 0 }
    }

    pub const fn stock_multiplayer() -> Self {
        Self::stock_campaign()
            .with_bootstrap(BootstrapRuleProfile::Multiplayer)
            .with_allow_win_tags(AllowWinTagsPolicy::Ignore)
            .with_computer_paranoid(ComputerParanoidPolicy::React)
            .with_crates(AuthoredCratePolicy::Skip)
            .with_ai_base(AiBasePolicy::ReplaceUnavailableNodes)
            .with_ai_mcv_deploy_hunt(true)
            .with_double_owned(DoubleOwnedPolicy::AllCountries)
            .with_mod_map_additions(true)
            .with_team_creation_limit_scope(TeamLimitScope::PerHouse)
            .with_dissolve_unfilled_teams(true)
    }

    const fn has_flag(self, flag: u32) -> bool {
        self.flags & flag != 0
    }

    const fn with_flag(mut self, flag: u32, enabled: bool) -> Self {
        if enabled {
            self.flags |= flag;
        } else {
            self.flags &= !flag;
        }
        self
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

    /// A computer's construction-yard maker can hunt to deploy, both from
    /// UnitClass::AI (0x007363DE..0x0073645B) and after an initial Unload
    /// attempt without a pending deploy (0x0073DDCB..0x0073DE1E).
    pub(crate) const fn allows_ai_mcv_deploy_hunt(self) -> bool {
        self.has_flag(Self::AI_MCV_DEPLOY_HUNT)
    }

    pub const fn with_ai_mcv_deploy_hunt(self, enabled: bool) -> Self {
        self.with_flag(Self::AI_MCV_DEPLOY_HUNT, enabled)
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
