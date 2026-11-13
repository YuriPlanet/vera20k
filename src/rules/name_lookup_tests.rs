//! A lookup by type name within a TechnoType array borrows an uppercase
//! name instead of copying it, and still ignores case.

use crate::rules::ini_parser::IniFile;
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;

#[test]
fn a_lookup_by_an_uppercase_type_name_copies_nothing() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=GAPOWR\n1=NAPOWR\n[VehicleTypes]\n0=DeathDummy\n\
         [GAPOWR]\n[NAPOWR]\n[DeathDummy]\n",
    ))
    .expect("fixture rules");
    let before = RuleSet::name_key_copies();

    assert_eq!(
        rules.type_array_index(ObjectCategory::Building, "NAPOWR"),
        Some(1)
    );
    assert_eq!(
        rules.building_type_at(0).map(|ty| ty.id.as_str()),
        Some("GAPOWR")
    );
    assert!(rules.task_force_member_object("NAPOWR").is_none());
    assert!(rules.object("GAPOWR").is_some());
    assert_eq!(RuleSet::name_key_copies(), before);

    // A name with a lowercase letter still resolves, through one copy each.
    assert_eq!(
        rules
            .object_in_category(ObjectCategory::Building, "gapowr")
            .map(|ty| ty.id.as_str()),
        Some("GAPOWR")
    );
    assert_eq!(
        rules.type_array_index(ObjectCategory::Vehicle, "DeathDummy"),
        Some(0)
    );
    assert_eq!(RuleSet::name_key_copies(), before + 2);
}
