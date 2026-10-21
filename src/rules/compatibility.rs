//! Build capability recorded by the retained Rules owner and save metadata.
//!
//! This is a VERA compatibility boundary, not a field from gamemd or Ares.
//! Keeping both wire variants lets a binary explicitly refuse the other mode.
//! Extension catalogs added later belong to the retained Rules registry, not
//! a second mutable policy on RuleSet or Simulation.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RulesCompatibility {
    Retail,
    Ares,
}

impl Default for RulesCompatibility {
    fn default() -> Self {
        if super::ARES_ENABLED {
            Self::Ares
        } else {
            Self::Retail
        }
    }
}

impl RulesCompatibility {
    pub(crate) fn matches_build(self) -> bool {
        self == Self::default()
    }
}

#[cfg(test)]
#[path = "compatibility_tests.rs"]
mod tests;
