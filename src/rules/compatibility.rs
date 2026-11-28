//! Build capability recorded by the retained Rules owner and save metadata.
//!
//! This is a VERA compatibility boundary, not a field from gamemd, Ares,
//! Phobos or NPatch. Keeping one wire variant per compiled capability
//! combination lets a binary explicitly refuse any other mode. Extension
//! catalogs added later belong to the retained Rules registry, not a second
//! mutable policy on RuleSet or Simulation.
//!
//! The capabilities are orthogonal: Phobos, NPatch and VeraX compile
//! independently and combine freely, so every combination needs its own wire
//! variant. A feature added under one capability must keep working when the
//! others are compiled in — the modes differ in what they read, never in
//! whether a feature is available.
//!
//! Ares, NPatch and its NPExt bug-fix chains are per-game switches now
//! (`RuleSet::ares`, `RuleSet::npatch`): every build carries their code, so
//! they are not build capabilities. Their variants stay, so a save from an
//! `ares` or `npatch` build still reads and is refused; no build records
//! them any more.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RulesCompatibility {
    Retail,
    Ares,
    Phobos,
    Npatch,
    AresPhobos,
    AresNpatch,
    PhobosNpatch,
    AresPhobosNpatch,
    // Append capability variants to retain existing discriminants and hashes.
    VeraX,
    AresVeraX,
    PhobosVeraX,
    NpatchVeraX,
    AresPhobosVeraX,
    AresNpatchVeraX,
    PhobosNpatchVeraX,
    AresPhobosNpatchVeraX,
}

impl Default for RulesCompatibility {
    fn default() -> Self {
        // NPatch is a per-game switch now (`rules::npatch_switch`), like Ares:
        // every build carries its code, so it is not a build capability. The
        // variants below stay for saves a build with the old `npatch` feature
        // wrote.
        Self::from_capabilities(false, super::PHOBOS_ENABLED, false, super::VERAX_ENABLED)
    }
}

impl RulesCompatibility {
    pub const fn from_capabilities(ares: bool, phobos: bool, npatch: bool, verax: bool) -> Self {
        match (ares, phobos, npatch, verax) {
            (true, true, true, true) => Self::AresPhobosNpatchVeraX,
            (true, true, true, false) => Self::AresPhobosNpatch,
            (true, true, false, true) => Self::AresPhobosVeraX,
            (true, true, false, false) => Self::AresPhobos,
            (true, false, true, true) => Self::AresNpatchVeraX,
            (true, false, true, false) => Self::AresNpatch,
            (true, false, false, true) => Self::AresVeraX,
            (true, false, false, false) => Self::Ares,
            (false, true, true, true) => Self::PhobosNpatchVeraX,
            (false, true, true, false) => Self::PhobosNpatch,
            (false, true, false, true) => Self::PhobosVeraX,
            (false, true, false, false) => Self::Phobos,
            (false, false, true, true) => Self::NpatchVeraX,
            (false, false, true, false) => Self::Npatch,
            (false, false, false, true) => Self::VeraX,
            (false, false, false, false) => Self::Retail,
        }
    }

    pub(crate) fn matches_build(self) -> bool {
        self == Self::default()
    }

    /// Whether this recorded mode includes the Ares capability.
    pub const fn has_ares(self) -> bool {
        matches!(
            self,
            Self::Ares
                | Self::AresPhobos
                | Self::AresNpatch
                | Self::AresVeraX
                | Self::AresPhobosNpatch
                | Self::AresPhobosVeraX
                | Self::AresNpatchVeraX
                | Self::AresPhobosNpatchVeraX
        )
    }

    /// Whether this recorded mode includes the Phobos capability.
    pub const fn has_phobos(self) -> bool {
        matches!(
            self,
            Self::Phobos
                | Self::AresPhobos
                | Self::PhobosNpatch
                | Self::PhobosVeraX
                | Self::AresPhobosNpatch
                | Self::AresPhobosVeraX
                | Self::PhobosNpatchVeraX
                | Self::AresPhobosNpatchVeraX
        )
    }

    /// Whether this recorded mode includes the NPatch capability (only a
    /// build from before NPatch became a per-game switch records it).
    pub const fn has_npatch(self) -> bool {
        matches!(
            self,
            Self::Npatch
                | Self::AresNpatch
                | Self::PhobosNpatch
                | Self::NpatchVeraX
                | Self::AresPhobosNpatch
                | Self::AresNpatchVeraX
                | Self::PhobosNpatchVeraX
                | Self::AresPhobosNpatchVeraX
        )
    }

    /// Whether this recorded mode includes the VeraX capability.
    pub const fn has_verax(self) -> bool {
        matches!(
            self,
            Self::VeraX
                | Self::AresVeraX
                | Self::PhobosVeraX
                | Self::NpatchVeraX
                | Self::AresPhobosVeraX
                | Self::AresNpatchVeraX
                | Self::PhobosNpatchVeraX
                | Self::AresPhobosNpatchVeraX
        )
    }
}

#[cfg(all(test, tests_rules_a_n))]
#[path = "compatibility_tests.rs"]
mod tests;
