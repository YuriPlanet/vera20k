//! Phobos `WarheadTypeExt` ReturnWarhead (`Ext/WarheadType/Body.cpp:235-241`,
//! `Detonate.cpp:610-636`): a warhead detonation that can strike the original
//! firer back.
//!
//! `ReturnWarhead=` names the warhead to send; `.Damage` (default 0),
//! `.Chance` (binary32 via `Parser<double>`, default 1.0), `.FullDetonation`
//! (default true), `.AffectsTarget` (`AffectedTarget`, default All) and
//! `.AffectsHouse` (`AffectedHouse`, default All) tune it.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::phobos_crit::{AffectedTarget, read_affected_target};
use crate::rules::phobos_shields::AffectedHouse;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PhobosReturnWarhead {
    pub warhead: Option<String>,
    pub damage: i32,
    pub chance: f64,
    pub full_detonation: bool,
    pub affects_target: AffectedTarget,
    pub affects_house: AffectedHouse,
}

impl Default for PhobosReturnWarhead {
    fn default() -> Self {
        Self {
            warhead: None,
            damage: 0,
            chance: 1.0,
            full_detonation: true,
            affects_target: AffectedTarget::ALL,
            affects_house: AffectedHouse::ALL,
        }
    }
}

impl PhobosReturnWarhead {
    pub fn from_section(section: &IniSection) -> Self {
        let defaults = Self::default();
        Self {
            warhead: section
                .read_type_name("ReturnWarhead", 0x800)
                .map(str::to_owned),
            damage: section.read_int("ReturnWarhead.Damage", defaults.damage),
            chance: section.read_phobos_double("ReturnWarhead.Chance", defaults.chance),
            full_detonation: section
                .read_ares_bool("ReturnWarhead.FullDetonation", defaults.full_detonation),
            affects_target: read_affected_target(
                section,
                "ReturnWarhead.AffectsTarget",
                defaults.affects_target,
            ),
            affects_house: crate::rules::phobos_shields::read_affected_house(
                section,
                "ReturnWarhead.AffectsHouse",
                defaults.affects_house,
            ),
        }
    }
}
