//! `Units.*` reads (`Ext/BuildingType/Body.cpp:228-231`).
use super::PhobosUnitsRepair;
use crate::rules::ini_parser::IniFile;

fn read(text: &str) -> PhobosUnitsRepair {
    PhobosUnitsRepair::from_section(IniFile::from_str(text).section_or_empty("DEPOT"))
}

#[test]
fn units_repair_defaults_keep_the_native_globals() {
    let set = read("[DEPOT]\nUnitRepair=yes\n");
    assert_eq!(set, PhobosUnitsRepair::default());
    assert_eq!(set.repair_rate, None);
    assert_eq!(set.repair_step, None);
    assert_eq!(set.repair_percent, None);
    assert_eq!(set.use_repair_cost, None);
}

#[test]
fn units_repair_reads_rate_step_percent_and_cost_switch() {
    let set = read(
        "[DEPOT]\nUnits.RepairRate=0.032\nUnits.RepairStep=7\n\
         Units.RepairPercent=25%\nUnits.UseRepairCost=no\n",
    );
    assert!((set.repair_rate.unwrap() - 0.032).abs() < 1e-6);
    assert_eq!(set.repair_step, Some(7));
    assert!((set.repair_percent.unwrap() - 0.25).abs() < 1e-6);
    assert_eq!(set.use_repair_cost, Some(false));
}

/// `<default>` returns each field to its unset state.
#[test]
fn units_repair_default_token_clears_every_field() {
    let set = read(
        "[DEPOT]\nUnits.RepairRate=<default>\nUnits.RepairStep=<default>\n\
         Units.RepairPercent=<default>\nUnits.UseRepairCost=<default>\n",
    );
    assert_eq!(set, PhobosUnitsRepair::default());
}
