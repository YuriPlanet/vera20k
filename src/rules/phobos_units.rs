//! Phobos `Units.*` (`Ext/BuildingType/Body.cpp:228-231`).
//!
//! A building type can override the rates, step, cost share and cost switch
//! the native repair service uses for the units it services. Consumers:
//! `BuildingClass_Mission_Repair`'s hospital and unit-repair arms
//! (`Ext/Building/Hooks.cpp:582-625`), `TechnoClass_ReceiveCommand`'s REPAIR
//! handling (`0x6F4D1A`, `Hooks.cpp:626-668`) and the `UnitReload=` separate
//! repair loop (`0x44C836`, `Hooks.cpp:579-615`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The four `BuildingTypeExt` repair-service fields, all `Nullable` so an
/// unset key keeps the native global (`Ext/BuildingType/Body.h:78-81`,
/// constructor `205-208`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PhobosUnitsRepair {
    /// `Units.RepairRate=`: the `IRepairRate`/`URepairRate` replacement.
    pub repair_rate: Option<f64>,
    /// `Units.RepairStep=`: the `IRepairStep`/`RepairStep` replacement.
    pub repair_step: Option<i32>,
    /// `Units.RepairPercent=`: the `RepairPercent` factor in the cost.
    pub repair_percent: Option<f64>,
    /// `Units.UseRepairCost=`: whether the serviced techno pays; default the
    /// native rule (`infantry` pay nothing).
    pub use_repair_cost: Option<bool>,
}

impl PhobosUnitsRepair {
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            repair_rate: section.read_phobos_nullable_double("Units.RepairRate", None),
            repair_step: section.read_phobos_nullable_int("Units.RepairStep", None),
            repair_percent: section.read_phobos_nullable_double("Units.RepairPercent", None),
            use_repair_cost: section.read_phobos_nullable_bool("Units.UseRepairCost", None),
        }
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_units_tests.rs"]
mod tests;
