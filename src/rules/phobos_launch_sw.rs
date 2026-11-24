//! Phobos `WarheadTypeExt` LaunchSW (`Ext/WarheadType/Body.cpp:290-296`,
//! `Detonate.cpp:121-150`): a warhead that launches superweapons at its
//! impact.
//!
//! `LaunchSW=` is a `ValueableIdxVector<SuperWeaponTypeClass>` (names resolved
//! to the house's Super indices at parse; VERA keeps the names and resolves in
//! the consumer). `LaunchSW.RealLaunch` defaults true; `.IgnoreInhibitors`
//! false; `.IgnoreDesignators` true; `.DisplayMoney` false with
//! `.DisplayMoney.Houses` All and `.DisplayMoney.Offset` empty
//! (`Ext/WarheadType/Body.h:418-424`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::phobos_shields::AffectedHouse;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PhobosLaunchSw {
    /// `LaunchSW=`: superweapon type names in listed order.
    pub super_types: Vec<String>,
    pub real_launch: bool,
    pub ignore_inhibitors: bool,
    pub ignore_designators: bool,
    pub display_money: bool,
    pub display_money_houses: AffectedHouse,
    pub display_money_offset: [i32; 2],
}

impl Default for PhobosLaunchSw {
    fn default() -> Self {
        Self {
            super_types: Vec::new(),
            real_launch: true,
            ignore_inhibitors: false,
            ignore_designators: true,
            display_money: false,
            display_money_houses: AffectedHouse::ALL,
            display_money_offset: [0, 0],
        }
    }
}

impl PhobosLaunchSw {
    pub fn from_section(section: &IniSection) -> Self {
        let defaults = Self::default();
        Self {
            super_types: section
                .read_list("LaunchSW", 0x800)
                .map(|tokens| {
                    tokens
                        .into_iter()
                        .map(|token| {
                            token
                                .trim_matches(|c| matches!(c, ' ' | '\t' | '\r'))
                                .to_owned()
                        })
                        .filter(|token| !token.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            real_launch: section.read_ares_bool("LaunchSW.RealLaunch", defaults.real_launch),
            ignore_inhibitors: section
                .read_ares_bool("LaunchSW.IgnoreInhibitors", defaults.ignore_inhibitors),
            ignore_designators: section
                .read_ares_bool("LaunchSW.IgnoreDesignators", defaults.ignore_designators),
            display_money: section.read_ares_bool("LaunchSW.DisplayMoney", defaults.display_money),
            display_money_houses: crate::rules::phobos_shields::read_affected_house(
                section,
                "LaunchSW.DisplayMoney.Houses",
                defaults.display_money_houses,
            ),
            display_money_offset: section.read_phobos_int_pair(
                "LaunchSW.DisplayMoney.Offset",
                defaults.display_money_offset,
            ),
        }
    }
}
