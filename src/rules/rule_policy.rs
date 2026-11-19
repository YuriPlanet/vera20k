//! Rule choices projected from the admitted scenario and multiplayer mode.

/// Selects the native rule path used when building a scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum BootstrapRuleProfile {
    Campaign,
    Multiplayer,
}

impl BootstrapRuleProfile {
    pub const fn uses_multiplayer_rules(self) -> bool {
        matches!(self, Self::Multiplayer)
    }
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
}
