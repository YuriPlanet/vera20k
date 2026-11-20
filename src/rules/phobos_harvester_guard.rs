//! Phobos `Harvester.CanGuardArea=` / `Harvester.CanGuardArea.RequireTarget=`
//! (`Ext/UnitType/Body.cpp:34-35`, `Valueable<bool>` declarations
//! `Ext/UnitType/Body.h:19-20`, constructor `false` `:89-90`).
//!
//! Consumer: `FootClass_MissionAreaGuard_Miner @ 0x4D6D34`
//! (`Ext/Unit/Hooks.Harvester.cpp:71-90`). A human-owned Unit harvester whose
//! type sets `CanGuardArea` keeps Area Guard instead of switching to Harvest;
//! with `RequireTarget` it keeps it only when
//! `TargetAndEstimateDamage(Location, ThreatType::Area)` finds a threat.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The two `Harvester.CanGuardArea*` values.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub struct PhobosHarvesterGuard {
    /// `Harvester.CanGuardArea=`: the human-owned harvester may hold Area
    /// Guard.
    pub can_guard_area: bool,
    /// `Harvester.CanGuardArea.RequireTarget=`: only while the area threat
    /// scan finds a target.
    pub require_target: bool,
}

impl PhobosHarvesterGuard {
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            // `Valueable<bool>`: an absent or invalid value keeps `false`.
            can_guard_area: section.read_ares_bool("Harvester.CanGuardArea", false),
            require_target: section.read_ares_bool("Harvester.CanGuardArea.RequireTarget", false),
        }
    }
}
