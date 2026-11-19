//! Phobos `HealthBar.*` (`Ext/TechnoType/Body.cpp:761-764`).
//!
//! All four are `Valueable<bool>` with constructor default `false`, read once
//! per rules pass with the current value as the default.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The health-bar presentation switches a TechnoType may set.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub struct HealthBarSettings {
    /// `HealthBar.Hide=`: skip the whole health-bar draw (`0x6F64A9`).
    pub hide: bool,
    /// `HealthBar.HidePips=`: skip the type's pip strip (`0x6F6637`).
    pub hide_pips: bool,
    /// `HealthBar.Permanent=`: draw while unhovered and unselected, provided
    /// the cell is not shrouded to the viewer (`0x6F5E37`).
    pub permanent: bool,
    /// `HealthBar.Permanent.PipScale=`: keep the pip scale while unhovered and
    /// unselected (`0x6F67E8` / `0x6F6A58`).
    pub permanent_pip_scale: bool,
}

impl HealthBarSettings {
    pub fn from_section(section: &IniSection) -> Self {
        let defaults = Self::default();
        Self {
            hide: section.read_bool("HealthBar.Hide", defaults.hide),
            hide_pips: section.read_bool("HealthBar.HidePips", defaults.hide_pips),
            permanent: section.read_bool("HealthBar.Permanent", defaults.permanent),
            permanent_pip_scale: section
                .read_bool("HealthBar.Permanent.PipScale", defaults.permanent_pip_scale),
        }
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_health_bar_tests.rs"]
mod tests;
