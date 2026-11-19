//! Phobos `PenetratesTransport.*` (`Ext/WarheadType/Body.cpp:437-442`,
//! `Ext/TechnoType/Body.cpp:1174-1177`).
//!
//! A warhead whose `Level` exceeds the target transport's level damages its
//! passengers: `WarheadTypeExt::ApplyPenetratesTransport`
//! (`Ext/WarheadType/Detonate.cpp:780-930`, called from `DetonateOnOneUnit`
//! at `:233`). A pass-through roll gates the whole effect, then a fatal roll
//! selects full-strength damage; otherwise every eligible passenger (or one
//! random one) takes `ceil(damage * whMult * typeMult)`. A clean pass with no
//! passenger left plays `CleanSound`.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// `WarheadTypeExt`'s six PenetratesTransport fields (`Body.h:258-263`
/// constructors `:508-513`).
#[derive(Debug, Clone, PartialEq)]
pub struct PenetratesTransportWarheadRules {
    /// `PenetratesTransport.Level=`: 0 disables the mechanism.
    pub level: i32,
    /// `PenetratesTransport.PassThrough=`; `< 1` may skip the effect.
    pub pass_through: f64,
    /// `PenetratesTransport.FatalRate=`: the chance of full-strength damage.
    pub fatal_rate: f64,
    /// `PenetratesTransport.DamageMultiplier=` for the non-fatal damage.
    pub damage_multiplier: f64,
    /// `PenetratesTransport.DamageAll=`: every eligible passenger rather than
    /// one random passenger.
    pub damage_all: bool,
    /// `PenetratesTransport.CleanSound=` (`ValueableIdx<VocClass>`): played at
    /// the transport when the attack leaves no passenger.
    pub clean_sound: Option<String>,
}

impl Default for PenetratesTransportWarheadRules {
    fn default() -> Self {
        Self {
            level: 0,
            pass_through: 1.0,
            fatal_rate: 0.0,
            damage_multiplier: 1.0,
            damage_all: false,
            clean_sound: None,
        }
    }
}

impl PenetratesTransportWarheadRules {
    pub fn from_section(section: &IniSection) -> Self {
        let defaults = Self::default();
        Self {
            level: section.read_ares_int("PenetratesTransport.Level", defaults.level),
            pass_through: section
                .read_phobos_double("PenetratesTransport.PassThrough", defaults.pass_through),
            fatal_rate: section
                .read_phobos_double("PenetratesTransport.FatalRate", defaults.fatal_rate),
            damage_multiplier: section.read_phobos_double(
                "PenetratesTransport.DamageMultiplier",
                defaults.damage_multiplier,
            ),
            damage_all: section.read_bool("PenetratesTransport.DamageAll", defaults.damage_all),
            clean_sound: section
                .read_type_name("PenetratesTransport.CleanSound", 2048)
                .map(str::to_owned),
        }
    }
}

/// `TechnoTypeExt`'s four PenetratesTransport fields (`Body.h:416-419`,
/// constructors `:836-839`); the level is `Nullable<int>` against the
/// `[CombatDamage]` global.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenetratesTransportTechnoRules {
    /// `PenetratesTransport.Level=` per type; unset uses the global.
    pub level: Option<i32>,
    pub pass_through_multiplier: f64,
    pub fatal_rate_multiplier: f64,
    pub damage_multiplier: f64,
}

impl Default for PenetratesTransportTechnoRules {
    fn default() -> Self {
        Self {
            level: None,
            pass_through_multiplier: 1.0,
            fatal_rate_multiplier: 1.0,
            damage_multiplier: 1.0,
        }
    }
}

impl PenetratesTransportTechnoRules {
    pub fn from_section(section: &IniSection) -> Self {
        let defaults = Self::default();
        Self {
            level: section.read_phobos_nullable_int("PenetratesTransport.Level", None),
            pass_through_multiplier: section.read_phobos_double(
                "PenetratesTransport.PassThroughMultiplier",
                defaults.pass_through_multiplier,
            ),
            fatal_rate_multiplier: section.read_phobos_double(
                "PenetratesTransport.FatalRateMultiplier",
                defaults.fatal_rate_multiplier,
            ),
            damage_multiplier: section.read_phobos_double(
                "PenetratesTransport.DamageMultiplier",
                defaults.damage_multiplier,
            ),
        }
    }

    /// The effective level against the `[CombatDamage]` global.
    pub fn level(&self, global: i32) -> i32 {
        self.level.unwrap_or(global)
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_penetrates_transport_tests.rs"]
mod tests;
