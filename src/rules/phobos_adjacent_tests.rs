//! `Adjacent.*` reads (`Ext/BuildingType/Body.cpp:232-235`).
use super::PhobosAdjacent;
use crate::rules::ini_parser::IniFile;

fn read(text: &str) -> PhobosAdjacent {
    PhobosAdjacent::from_section(IniFile::from_str(text).section_or_empty("PLACER"))
}

#[test]
fn adjacent_defaults_keep_every_provider() {
    let set = read("[PLACER]\nFoundation=1x1\n");
    assert_eq!(set, PhobosAdjacent::default());
    assert!(set.allowed.is_empty() && set.disallowed.is_empty());
    assert!(!set.prohibit);
    assert_eq!(set.prohibit_distance, 0);
    assert!(!set.no_build_area_on_buildup);
    assert!(set.permits_provider("ANY"));
    assert!(!set.disallows("ANY"));
}

#[test]
fn adjacent_reads_lists_prohibit_and_distance() {
    let set = read(
        "[PLACER]\nAdjacent.Allowed=GAPOWR,GAREFN\nAdjacent.Disallowed=GAPOWR\n\
         Adjacent.Disallowed.Prohibit=yes\n\
         Adjacent.Disallowed.ProhibitDistance=4\n\
         NoBuildAreaOnBuildup=yes\n",
    );
    assert_eq!(set.allowed, ["GAPOWR", "GAREFN"]);
    assert_eq!(set.disallowed, ["GAPOWR"]);
    assert!(set.prohibit);
    assert_eq!(set.prohibit_distance, 4);
    assert!(set.no_build_area_on_buildup);
    assert!(set.permits_provider("GAREFN"));
    assert!(!set.permits_provider("GAPILE"));
    assert!(set.disallows("gapowr"));
}
