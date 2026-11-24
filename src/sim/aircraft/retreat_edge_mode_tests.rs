//! Phobos `RetreatToEdge=` / `[General] AircraftRetreatToEdge=` as
//! `AircraftClass::Mission_Retreat` (`0x00415A50`) reads them through
//! `AircraftExt::PickEdgeCellForPlane` (`Ext/Aircraft/Body.cpp:89`).

use super::retreat_edge_mode;
use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_aircraft_edge::PhobosEdgeType;
use crate::rules::ruleset::RuleSet;

fn rules_with(general: &str, type_body: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\n{general}\
         [AircraftTypes]\n0=PLANE\n\
         [PLANE]\nStrength=100\n{type_body}"
    )))
    .unwrap()
}

/// The type key beats the `[General]` fallback, an unset one inherits it, and
/// the constructor value is `Owner`.
#[test]
fn the_retreat_edge_mode_reads_the_type_then_the_general_key() {
    let typed = rules_with("AircraftRetreatToEdge=closest\n", "RetreatToEdge=random\n");
    let plane = typed.object("PLANE").unwrap();
    assert_eq!(
        retreat_edge_mode(&typed, Some(plane)),
        PhobosEdgeType::Random
    );

    let unset = rules_with("AircraftRetreatToEdge=closest\n", "");
    let plane = unset.object("PLANE").unwrap();
    assert_eq!(
        retreat_edge_mode(&unset, Some(plane)),
        PhobosEdgeType::Closest
    );

    let defaulted = rules_with("", "");
    let plane = defaulted.object("PLANE").unwrap();
    assert_eq!(
        retreat_edge_mode(&defaulted, Some(plane)),
        PhobosEdgeType::Owner
    );

    // A rejected spelling keeps the constructor value rather than failing.
    let bad = rules_with("AircraftRetreatToEdge=nearest\n", "RetreatToEdge=nearest\n");
    let plane = bad.object("PLANE").unwrap();
    assert_eq!(retreat_edge_mode(&bad, Some(plane)), PhobosEdgeType::Owner);
}
