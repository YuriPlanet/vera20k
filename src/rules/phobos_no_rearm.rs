//! Phobos `NoRearm.*` / `NoReload.*` (`Ext/TechnoType/Body.cpp:1054-1057`).
//!
//! Four `Nullable<bool>` techno-type keys: while a techno is EMP'd or
//! deactivated (`TechnoExt::UpdateRearmInEMPState`,
//! `Ext/Techno/Body.Update.cpp:866-880`) or held by a Temporal weapon
//! (`UpdateRearmInTemporal`, `:882-895`), a `yes` key holds the matching
//! timer still with `StartTime++` on every visit. An unset key falls back to
//! the `[General]` global (`Ext/Rules/Body.cpp:383-386`, constructor
//! `Ext/Rules/Body.h:804-807`), through `Nullable<bool>::Get`.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The four `TechnoTypeExt` fields, each `Nullable<bool>` constructed unset
/// (`Ext/TechnoType/Body.h:286-289`, constructor `:718-721`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PhobosNoRearm {
    /// `NoRearm.UnderEMP=`.
    pub rearm_under_emp: Option<bool>,
    /// `NoRearm.Temporal=`.
    pub rearm_temporal: Option<bool>,
    /// `NoReload.UnderEMP=`.
    pub reload_under_emp: Option<bool>,
    /// `NoReload.Temporal=`.
    pub reload_temporal: Option<bool>,
}

impl PhobosNoRearm {
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            rearm_under_emp: section.read_phobos_nullable_bool("NoRearm.UnderEMP", None),
            rearm_temporal: section.read_phobos_nullable_bool("NoRearm.Temporal", None),
            reload_under_emp: section.read_phobos_nullable_bool("NoReload.UnderEMP", None),
            reload_temporal: section.read_phobos_nullable_bool("NoReload.Temporal", None),
        }
    }

    /// `Nullable<bool>::Get(global)`: the type value when set, else the
    /// `[General]` global's `false` default.
    pub fn rearm_under_emp(&self, global: bool) -> bool {
        self.rearm_under_emp.unwrap_or(global)
    }

    /// The `NoRearm.Temporal=` effective value.
    pub fn rearm_temporal(&self, global: bool) -> bool {
        self.rearm_temporal.unwrap_or(global)
    }

    /// The `NoReload.UnderEMP=` effective value.
    pub fn reload_under_emp(&self, global: bool) -> bool {
        self.reload_under_emp.unwrap_or(global)
    }

    /// The `NoReload.Temporal=` effective value.
    pub fn reload_temporal(&self, global: bool) -> bool {
        self.reload_temporal.unwrap_or(global)
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_no_rearm_tests.rs"]
mod tests;
