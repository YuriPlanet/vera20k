//! Phobos `RateDown.*` (`Ext/TechnoType/Body.cpp:1049-1052`).
//!
//! The values extend the native gattling decay: a delay before the value
//! starts falling, an optional full reset when the target changes, and a
//! replacement decay rate while ammo is at or below a threshold. Consumers:
//! `TechnoClass::DecreaseValue` / `UpdateGattlingStage` hooks
//! (`Ext/Techno/Hooks.Misc.cpp:652-745`) and `UpdateGattlingRateDownReset`
//! (`Ext/Techno/Body.Update.cpp:568`) from the target setter.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The four `TechnoTypeExt` rate-down fields, with the constructor defaults
/// `Delay 0`, `Reset false`, `Cover.Value 0`, `Cover.AmmoBelow -2`
/// (`Ext/TechnoType/Body.h:713-716`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhobosRateDown {
    /// `RateDown.Delay=`: frames before the decay resumes; negative disables
    /// the decay entirely.
    pub delay: i32,
    /// `RateDown.Reset=`: zero the gattling value when the target changes or
    /// is lost.
    pub reset: bool,
    /// `RateDown.Cover.Value=`: decay amount while ammo is at or below
    /// `cover_ammo_below`.
    pub cover_value: i32,
    /// `RateDown.Cover.AmmoBelow=`.
    pub cover_ammo_below: i32,
}

impl Default for PhobosRateDown {
    fn default() -> Self {
        Self {
            delay: 0,
            reset: false,
            cover_value: 0,
            cover_ammo_below: -2,
        }
    }
}

impl PhobosRateDown {
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            delay: section.read_ares_int("RateDown.Delay", 0),
            reset: section.read_ares_bool("RateDown.Reset", false),
            cover_value: section.read_ares_int("RateDown.Cover.Value", 0),
            cover_ammo_below: section.read_ares_int("RateDown.Cover.AmmoBelow", -2),
        }
    }

    /// The decay amount for a techno with `ammo`.
    pub fn rate_down(&self, stock_rate: i32, ammo: i32) -> i32 {
        if ammo <= self.cover_ammo_below {
            self.cover_value
        } else {
            stock_rate
        }
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_rate_down_tests.rs"]
mod tests;
