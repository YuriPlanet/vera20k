//! Phobos `DrainMoneyDisplay.*` (`Ext/TechnoType/Body.cpp:1165-1169`).
//!
//! The four type keys shape the flying credit labels of a DrainWeapon link
//! (`Ext/Techno/Hooks.cpp:199-219`, hook `0x6FA167`): whether the drainer
//! shows the money it grabs, to which Houses, with what pixel offset, and
//! whether the drained building also shows the loss. The consumer is
//! [`crate::sim::credit_income::drain_common_step`].
//!
//! The `UseDisplayIncome` arm passes the drained building's own
//! `DisplayIncome.Houses`/`.Offset` (`Ext/BuildingType/Body.cpp:216-217`) to
//! the same label, so those two reads are ported here as well. The rest of the
//! `DisplayIncome` feature (accumulated income and its own periodic label) is a
//! separate chain and is not read.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::phobos_shields::AffectedHouse;

/// The five `TechnoTypeExt` drain-display fields. All `Nullable` except the
/// offset (`Valueable<Point2D>`), so an unset key falls back to the
/// `[AudioVisual]` global and an unset offset is `0,0`
/// (`Ext/TechnoType/Body.h:405-409`, constructor `:827-831`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrainMoneyDisplaySettings {
    /// `DrainMoneyDisplay=`: the drainer-side label (`Body.cpp:1165`).
    pub display: Option<bool>,
    /// `DrainMoneyDisplay.Houses=`: the `AffectedHouse` filter (`:1166`).
    pub houses: Option<AffectedHouse>,
    /// `DrainMoneyDisplay.Offset=`: the label's screen-pixel offset (`:1167`).
    pub offset: [i32; 2],
    /// `DrainMoneyDisplay.OnTarget=`: the victim-side label (`:1168`).
    pub on_target: Option<bool>,
    /// `DrainMoneyDisplay.OnTarget.UseDisplayIncome=`: use the building's
    /// `DisplayIncome.*` filter and offset instead of the drain keys (`:1169`).
    pub on_target_use_display_income: Option<bool>,
}

impl Default for DrainMoneyDisplaySettings {
    fn default() -> Self {
        Self {
            display: None,
            houses: None,
            offset: [0, 0],
            on_target: None,
            on_target_use_display_income: None,
        }
    }
}

impl DrainMoneyDisplaySettings {
    /// The reads of `TechnoTypeExt::LoadFromINIFile`
    /// (`Ext/TechnoType/Body.cpp:1165-1169`), in native order. Each key is
    /// independent, so one read of the merged layer state is equivalent.
    pub fn from_section(section: &IniSection) -> Self {
        let defaults = Self::default();
        Self {
            display: section.read_phobos_nullable_bool("DrainMoneyDisplay", defaults.display),
            houses: section.read_phobos_nullable_with(
                "DrainMoneyDisplay.Houses",
                defaults.houses,
                AffectedHouse::parse,
            ),
            offset: section.read_phobos_int_pair("DrainMoneyDisplay.Offset", defaults.offset),
            on_target: section
                .read_phobos_nullable_bool("DrainMoneyDisplay.OnTarget", defaults.on_target),
            on_target_use_display_income: section.read_phobos_nullable_bool(
                "DrainMoneyDisplay.OnTarget.UseDisplayIncome",
                defaults.on_target_use_display_income,
            ),
        }
    }
}

/// The `BuildingTypeExt` display-income keys (`Ext/BuildingType/Body.cpp:
/// 208-217`, `Body.h:46-49`): `DisplayIncome` and `.Delay` are `Nullable`,
/// `Houses` is `Nullable<AffectedHouse>`, `Offset` a `Valueable<Point2D>`
/// defaulting to `0,0`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DisplayIncomeSettings {
    /// `DisplayIncome=`, falling back to the global switch.
    pub enabled: Option<bool>,
    /// `DisplayIncome.Delay=`: frames between labels; an authored zero is
    /// forced to one (`Body.cpp:211-214`).
    pub delay: Option<i32>,
    /// `DisplayIncome.Houses=`.
    pub houses: Option<AffectedHouse>,
    /// `DisplayIncome.Offset=`.
    pub offset: [i32; 2],
}

impl DisplayIncomeSettings {
    pub fn from_section(section: &IniSection) -> Self {
        let defaults = Self::default();
        let delay = section
            .read_phobos_nullable_int("DisplayIncome.Delay", defaults.delay)
            .map(|delay| if delay == 0 { 1 } else { delay });
        Self {
            enabled: section.read_phobos_nullable_bool("DisplayIncome", defaults.enabled),
            delay,
            houses: section.read_phobos_nullable_with(
                "DisplayIncome.Houses",
                defaults.houses,
                AffectedHouse::parse,
            ),
            offset: section.read_phobos_int_pair("DisplayIncome.Offset", defaults.offset),
        }
    }
}

#[cfg(test)]
#[path = "phobos_drain_money_display_tests.rs"]
mod tests;
