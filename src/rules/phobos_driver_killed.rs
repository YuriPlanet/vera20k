//! Phobos `DriverKilled.KeptPassengers=` / `DriverKilled.KillPassengers=`
//! type keys (`Ext/TechnoType/Body.cpp:969-970`, `Nullable<bool>` fields
//! `Ext/TechnoType/Body.h:212-213`, constructor unset `:641-642`) and their
//! `[CombatDamage]` globals (`Ext/Rules/Body.cpp:533-534`, `Valueable<bool>`
//! fields `Ext/Rules/Body.h:949-950`, both construct `false`).
//!
//! Consumer: `TechnoExt::ApplyKillDriver`
//! (`Ext/Techno/Body.cpp:930-1017`, the Ares `100458F3..10045953`
//! driver-change body). After the operator removal,
//! `passive && KeptPassengers.Get(global)` breaks out and keeps every
//! passenger aboard; otherwise `KillPassengers.Get(global)` turns each
//! departing passenger into a registered destruction instead of an
//! `EjectRandomly` attempt. `Nullable<T>::Get` returns the type value when
//! set, else the global; the globals have no unset state.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The two `DriverKilled.*` keys on one TechnoType.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PhobosDriverKilled {
    /// `DriverKilled.KeptPassengers=`: with a neutral new owner, keep the
    /// remaining passengers aboard instead of ejecting or killing them.
    /// `None` falls back to the `[CombatDamage]` global.
    pub kept_passengers: Option<bool>,
    /// `DriverKilled.KillPassengers=`: destroy each departing passenger
    /// instead of attempting its random ejection. `None` falls back to the
    /// `[CombatDamage]` global.
    pub kill_passengers: Option<bool>,
}

impl Default for PhobosDriverKilled {
    fn default() -> Self {
        Self {
            kept_passengers: None,
            kill_passengers: None,
        }
    }
}

impl PhobosDriverKilled {
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            kept_passengers: section.read_phobos_nullable_bool("DriverKilled.KeptPassengers", None),
            kill_passengers: section.read_phobos_nullable_bool("DriverKilled.KillPassengers", None),
        }
    }

    /// `DriverKilled_KeptPassengers.Get(RulesExt::Global()->...)`.
    pub fn kept_passengers(&self, global: bool) -> bool {
        self.kept_passengers.unwrap_or(global)
    }

    /// `DriverKilled_KillPassengers.Get(RulesExt::Global()->...)`.
    pub fn kill_passengers(&self, global: bool) -> bool {
        self.kill_passengers.unwrap_or(global)
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_driver_killed_tests.rs"]
mod tests;
