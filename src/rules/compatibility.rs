//! Build capability recorded by the retained Rules owner and save metadata.
//!
//! This is a VERA compatibility boundary, not a field from gamemd, Ares,
//! Phobos or NPatch. Keeping one wire variant per compiled capability
//! combination lets a binary explicitly refuse any other mode. Extension
//! catalogs added later belong to the retained Rules registry, not a second
//! mutable policy on RuleSet or Simulation.
//!
//! The five capabilities are orthogonal: Ares, Phobos, NPatch, VeraX and the
//! NPBUG bug-fix extension compile independently and combine freely, so every
//! combination needs its own wire variant. A feature added under one capability
//! must keep working when the others are compiled in — the modes differ in what
//! they read, never in whether a feature is available.

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
    Npbug,
    AresNpbug,
    PhobosNpbug,
    NpatchNpbug,
    VeraXNpbug,
    AresPhobosNpbug,
    AresNpatchNpbug,
    AresVeraXNpbug,
    PhobosNpatchNpbug,
    PhobosVeraXNpbug,
    NpatchVeraXNpbug,
    AresPhobosNpatchNpbug,
    AresPhobosVeraXNpbug,
    AresNpatchVeraXNpbug,
    PhobosNpatchVeraXNpbug,
    AresPhobosNpatchVeraXNpbug,
}

impl Default for RulesCompatibility {
    fn default() -> Self {
        Self::from_capabilities(
            super::ARES_BUILD,
            super::PHOBOS_ENABLED,
            super::NPATCH_ENABLED,
            super::VERAX_ENABLED,
            super::NPBUG_ENABLED,
        )
    }
}

impl RulesCompatibility {
    pub const fn from_capabilities(
        ares: bool,
        phobos: bool,
        npatch: bool,
        verax: bool,
        npbug: bool,
    ) -> Self {
        match (ares, phobos, npatch, verax, npbug) {
            (true, true, true, true, false) => Self::AresPhobosNpatchVeraX,
            (true, true, true, false, false) => Self::AresPhobosNpatch,
            (true, true, false, true, false) => Self::AresPhobosVeraX,
            (true, true, false, false, false) => Self::AresPhobos,
            (true, false, true, true, false) => Self::AresNpatchVeraX,
            (true, false, true, false, false) => Self::AresNpatch,
            (true, false, false, true, false) => Self::AresVeraX,
            (true, false, false, false, false) => Self::Ares,
            (false, true, true, true, false) => Self::PhobosNpatchVeraX,
            (false, true, true, false, false) => Self::PhobosNpatch,
            (false, true, false, true, false) => Self::PhobosVeraX,
            (false, true, false, false, false) => Self::Phobos,
            (false, false, true, true, false) => Self::NpatchVeraX,
            (false, false, true, false, false) => Self::Npatch,
            (false, false, false, true, false) => Self::VeraX,
            (false, false, false, false, false) => Self::Retail,
            (true, true, true, true, true) => Self::AresPhobosNpatchVeraXNpbug,
            (true, true, true, false, true) => Self::AresPhobosNpatchNpbug,
            (true, true, false, true, true) => Self::AresPhobosVeraXNpbug,
            (true, true, false, false, true) => Self::AresPhobosNpbug,
            (true, false, true, true, true) => Self::AresNpatchVeraXNpbug,
            (true, false, true, false, true) => Self::AresNpatchNpbug,
            (true, false, false, true, true) => Self::AresVeraXNpbug,
            (true, false, false, false, true) => Self::AresNpbug,
            (false, true, true, true, true) => Self::PhobosNpatchVeraXNpbug,
            (false, true, true, false, true) => Self::PhobosNpatchNpbug,
            (false, true, false, true, true) => Self::PhobosVeraXNpbug,
            (false, true, false, false, true) => Self::PhobosNpbug,
            (false, false, true, true, true) => Self::NpatchVeraXNpbug,
            (false, false, true, false, true) => Self::NpatchNpbug,
            (false, false, false, true, true) => Self::VeraXNpbug,
            (false, false, false, false, true) => Self::Npbug,
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
                | Self::AresNpbug
                | Self::AresPhobosNpbug
                | Self::AresNpatchNpbug
                | Self::AresVeraXNpbug
                | Self::AresPhobosNpatchNpbug
                | Self::AresPhobosVeraXNpbug
                | Self::AresNpatchVeraXNpbug
                | Self::AresPhobosNpatchVeraXNpbug
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
                | Self::PhobosNpbug
                | Self::AresPhobosNpbug
                | Self::PhobosNpatchNpbug
                | Self::PhobosVeraXNpbug
                | Self::AresPhobosNpatchNpbug
                | Self::AresPhobosVeraXNpbug
                | Self::PhobosNpatchVeraXNpbug
                | Self::AresPhobosNpatchVeraXNpbug
        )
    }

    /// Whether this recorded mode includes the NPatch capability.
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
                | Self::NpatchNpbug
                | Self::AresNpatchNpbug
                | Self::PhobosNpatchNpbug
                | Self::NpatchVeraXNpbug
                | Self::AresPhobosNpatchNpbug
                | Self::AresNpatchVeraXNpbug
                | Self::PhobosNpatchVeraXNpbug
                | Self::AresPhobosNpatchVeraXNpbug
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
                | Self::VeraXNpbug
                | Self::AresVeraXNpbug
                | Self::PhobosVeraXNpbug
                | Self::NpatchVeraXNpbug
                | Self::AresPhobosVeraXNpbug
                | Self::AresNpatchVeraXNpbug
                | Self::PhobosNpatchVeraXNpbug
                | Self::AresPhobosNpatchVeraXNpbug
        )
    }

    /// Whether this recorded mode includes the NPBUG bug-fix capability.
    pub const fn has_npbug(self) -> bool {
        matches!(
            self,
            Self::Npbug
                | Self::AresNpbug
                | Self::PhobosNpbug
                | Self::NpatchNpbug
                | Self::VeraXNpbug
                | Self::AresPhobosNpbug
                | Self::AresNpatchNpbug
                | Self::AresVeraXNpbug
                | Self::PhobosNpatchNpbug
                | Self::PhobosVeraXNpbug
                | Self::NpatchVeraXNpbug
                | Self::AresPhobosNpatchNpbug
                | Self::AresPhobosVeraXNpbug
                | Self::AresNpatchVeraXNpbug
                | Self::PhobosNpatchVeraXNpbug
                | Self::AresPhobosNpatchVeraXNpbug
        )
    }
}

#[cfg(test)]
#[path = "compatibility_tests.rs"]
mod tests;
