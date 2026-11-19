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
    pub(crate) const fn from_nonzero(nonzero: bool) -> Self {
        if nonzero {
            Self::Multiplayer
        } else {
            Self::Campaign
        }
    }

    pub const fn uses_multiplayer_rules(self) -> bool {
        matches!(self, Self::Multiplayer)
    }

    /// The mode-zero arm of `ReadMapOverlayPacks` keeps authored crate rows.
    pub(crate) const fn loads_authored_crates(self) -> bool {
        matches!(self, Self::Campaign)
    }

    pub(crate) const fn forces_listed_ai_trigger_enables(self) -> bool {
        self.uses_multiplayer_rules()
    }

    pub(crate) const fn uses_default_task_force_member_tech_level(self) -> bool {
        self.uses_multiplayer_rules()
    }
}

// Existing engine fixtures describe startup with a zero/nonzero flag.
#[cfg(any(test, feature = "test-support"))]
impl From<bool> for BootstrapRuleProfile {
    fn from(nonzero: bool) -> Self {
        Self::from_nonzero(nonzero)
    }
}

/// The set of live TeamClass instances that count toward a TeamType's `Max=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TeamLimitScope {
    AllHouses,
    PerHouse,
}

/// Built-in rules are derived from the saved session mode and mode class.
/// The session persists and hashes those inputs once. Network-only behavior
/// also checks the session's execution topology at the point of use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub struct RulePolicy {
    bootstrap: BootstrapRuleProfile,
    shared_allied_victory: bool,
    skips_network_score_dialog: bool,
}

const _: () = assert!(std::mem::size_of::<RulePolicy>() <= 16);

impl RulePolicy {
    pub(crate) const fn new(
        bootstrap: BootstrapRuleProfile,
        shared_allied_victory: bool,
        skips_network_score_dialog: bool,
    ) -> Self {
        Self {
            bootstrap,
            shared_allied_victory,
            skips_network_score_dialog,
        }
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
        matches!(self.bootstrap, BootstrapRuleProfile::Campaign)
    }

    /// `TeamTypeClass::Create_Team @ 0x006F09C0` checks mode at 0x006F0A16.
    pub(crate) const fn team_creation_limit_scope(self) -> TeamLimitScope {
        match self.bootstrap {
            BootstrapRuleProfile::Campaign => TeamLimitScope::AllHouses,
            BootstrapRuleProfile::Multiplayer => TeamLimitScope::PerHouse,
        }
    }

    /// `TeamClass::AI @ 0x006E9140` uses `DissolveUnfilledTeamDelay=` for
    /// never-full empty teams in the nonzero mode arm at `0x006E92AD`.
    pub(crate) const fn unfilled_team_timeout(self, delay: i32) -> Option<i32> {
        match self.bootstrap {
            BootstrapRuleProfile::Campaign => None,
            BootstrapRuleProfile::Multiplayer => Some(delay),
        }
    }
}
