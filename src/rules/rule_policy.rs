//! Rule choices projected from the admitted scenario and multiplayer mode.

/// Selects the native rule path used when building a scenario.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[repr(u8)]
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
#[repr(u8)]
pub enum AllowWinTagsPolicy {
    RequireClear,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum ComputerParanoidPolicy {
    Disabled,
    React,
}

/// Admission of map-authored crate overlays at load time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum AuthoredCratePolicy {
    Keep,
    Skip,
}

/// Whether the AI may replace unavailable or invalidated base-plan nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum AiBasePolicy {
    RetainAuthoredNodes,
    ReplaceUnavailableNodes,
}

/// How factory selection interprets `DoubleOwned=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum DoubleOwnedPolicy {
    OwnerMask,
    AllCountries,
}

/// The set of live TeamClass instances that count toward a TeamType's `Max=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub(crate) enum TeamLimitScope {
    AllHouses,
    PerHouse,
}

/// Independently selectable rules. Stock live sessions derive their preset
/// from the saved mode and mode class. Network-only behavior separately
/// checks the session's execution topology at the point of use. Offline
/// Skirmish (GameMode 5) uses the multiplayer preset with local execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub struct RulePolicy {
    bootstrap: BootstrapRuleProfile,
    pub allow_win_tags: AllowWinTagsPolicy,
    pub computer_paranoid: ComputerParanoidPolicy,
    pub crates: AuthoredCratePolicy,
    pub ai_base: AiBasePolicy,
    pub double_owned: DoubleOwnedPolicy,
    pub mod_map_additions: bool,
    team_limit: TeamLimitScope,
    dissolve_unfilled_teams: bool,
    shared_allied_victory: bool,
    skips_network_score_dialog: bool,
}

const _: () = assert!(std::mem::size_of::<RulePolicy>() <= 16);

impl Default for RulePolicy {
    fn default() -> Self {
        Self::stock_campaign()
    }
}

impl RulePolicy {
    pub const fn stock_campaign() -> Self {
        Self {
            bootstrap: BootstrapRuleProfile::Campaign,
            allow_win_tags: AllowWinTagsPolicy::RequireClear,
            computer_paranoid: ComputerParanoidPolicy::Disabled,
            crates: AuthoredCratePolicy::Keep,
            ai_base: AiBasePolicy::RetainAuthoredNodes,
            double_owned: DoubleOwnedPolicy::OwnerMask,
            mod_map_additions: false,
            team_limit: TeamLimitScope::AllHouses,
            dissolve_unfilled_teams: false,
            shared_allied_victory: false,
            skips_network_score_dialog: false,
        }
    }

    pub const fn stock_multiplayer() -> Self {
        Self {
            bootstrap: BootstrapRuleProfile::Multiplayer,
            allow_win_tags: AllowWinTagsPolicy::Ignore,
            computer_paranoid: ComputerParanoidPolicy::React,
            crates: AuthoredCratePolicy::Skip,
            ai_base: AiBasePolicy::ReplaceUnavailableNodes,
            double_owned: DoubleOwnedPolicy::AllCountries,
            mod_map_additions: true,
            team_limit: TeamLimitScope::PerHouse,
            dissolve_unfilled_teams: true,
            shared_allied_victory: false,
            skips_network_score_dialog: false,
        }
    }

    pub(crate) const fn with_mode_outcomes(
        mut self,
        shared_allied_victory: bool,
        skips_network_score_dialog: bool,
    ) -> Self {
        self.shared_allied_victory = shared_allied_victory;
        self.skips_network_score_dialog = skips_network_score_dialog;
        self
    }

    pub const fn bootstrap(self) -> BootstrapRuleProfile {
        self.bootstrap
    }

    pub const fn shared_allied_victory(self) -> bool {
        self.shared_allied_victory
    }

    pub const fn skips_network_score_dialog(self) -> bool {
        self.skips_network_score_dialog
    }

    /// `HouseClass::Update @ 0x004F8440` checks the AllowWin count only
    /// in the mode-zero victory arm (`0x004F85F7..0x004F8605`).
    pub(crate) const fn victory_requires_cleared_allow_win_tags(self) -> bool {
        matches!(self.allow_win_tags, AllowWinTagsPolicy::RequireClear)
    }

    /// Admission of diplomacy and base requests into `Computer_Paranoid @
    /// 0x00501640`. Each caller applies its own House and Rules checks.
    pub(crate) const fn permits_paranoid_reactions(self) -> bool {
        matches!(self.computer_paranoid, ComputerParanoidPolicy::React)
    }

    /// `TeamTypeClass::Create_Team @ 0x006F09C0` checks mode at 0x006F0A16.
    pub(crate) const fn team_creation_limit_scope(self) -> TeamLimitScope {
        self.team_limit
    }

    /// `TeamClass::AI @ 0x006E9140` uses `DissolveUnfilledTeamDelay=` for
    /// never-full empty teams in the nonzero mode arm at `0x006E92AD`.
    pub(crate) const fn unfilled_team_timeout(self, delay: i32) -> Option<i32> {
        if self.dissolve_unfilled_teams {
            Some(delay)
        } else {
            None
        }
    }
}
