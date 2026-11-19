//! Phobos `OpenTransport.*` rider-side keys (`Ext/TechnoType/Body.cpp:890-892`,
//! declarations `Ext/TechnoType/Body.h:160-162,557-559`).
//!
//! The three `Nullable` values live on the *passenger* and supplement the
//! carrier's `OpenTopped.*` values:
//!
//! * `OpenTransport.RangeBonus=` (cells) adds to the carrier's
//!   `OpenTopped.RangeBonus` at `TechnoClass_IsCloseEnoughToTarget_OpenTopped_RangeBonus`
//!   (`0x6F72D2`, `Ext/Techno/Hooks.Transport.cpp:167-183`).
//! * `OpenTransport.DamageMultiplier=` (`Nullable<float>`) multiplies the
//!   passenger's firepower at `TechnoClass_FireAt_OpenToppedDmgMult`
//!   (`0x6FE43B`, `Ext/Techno/Hooks.Firing.cpp:714-741`) and in
//!   `TechnoExt::GetCurrentFirepowerMultiplier` (`Ext/Techno/Body.cpp:228-232`).
//! * `OpenTransport.FireWhileMoving=` joins the carrier's
//!   `OpenTopped.FireWhileMoving` and the weapon's `FireWhileMoving` in the
//!   firing gate (`0x6FC5C7`, `Ext/Techno/Hooks.Firing.cpp:439-482`; also
//!   `Hooks.Transport.cpp:915`).
//!
//! Globals: `[CombatDamage] OpenTransport.RangeBonus` (constructor 0) and
//! `OpenTransport.DamageMultiplier` (constructor 1.0), `[General]
//! OpenTransport.FireWhileMoving` (constructor true); declarations
//! `Ext/Rules/Body.h:373-375`, defaults `:878-880`, reads `Ext/Rules/Body.cpp:463-465`.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::ruleset::RuleSet;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PhobosOpenTransport {
    /// `OpenTransport.RangeBonus=` (`Body.cpp:890`), cells.
    pub range_bonus: Option<i32>,
    /// `OpenTransport.DamageMultiplier=` (`Body.cpp:891`), narrowed to f32.
    pub damage_multiplier: Option<f32>,
    /// `OpenTransport.FireWhileMoving=` (`Body.cpp:892`).
    pub fire_while_moving: Option<bool>,
}

impl PhobosOpenTransport {
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            range_bonus: section.read_phobos_nullable_int("OpenTransport.RangeBonus", None),
            damage_multiplier: section
                .read_phobos_nullable_double("OpenTransport.DamageMultiplier", None)
                .map(|value| value as f32),
            fire_while_moving: section
                .read_phobos_nullable_bool("OpenTransport.FireWhileMoving", None),
        }
    }

    /// `Nullable<int>::Get(RulesExt::Global()->OpenTransport_RangeBonus)`.
    pub fn range_bonus(&self, rules: &RuleSet) -> i32 {
        self.range_bonus
            .unwrap_or(rules.phobos_globals.open_transport_range_bonus)
    }

    /// `Nullable<float>::Get(RulesExt::Global()->OpenTransport_DamageMultiplier)`.
    pub fn damage_multiplier(&self, rules: &RuleSet) -> f32 {
        self.damage_multiplier.unwrap_or(f32::from_bits(
            rules.phobos_globals.open_transport_damage_multiplier.bits(),
        ))
    }

    /// `Nullable<bool>::Get(RulesExt::Global()->OpenTransport_FireWhileMoving)`.
    pub fn fire_while_moving(&self, rules: &RuleSet) -> bool {
        self.fire_while_moving
            .unwrap_or(rules.phobos_globals.open_transport_fire_while_moving)
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_open_transport_tests.rs"]
mod tests;
