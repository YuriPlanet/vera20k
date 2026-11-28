//! `RoofProductionAnim*` ART reads (`Ext/BuildingType/Body.cpp:325-335`).
use super::PhobosRoofProductionAnim;
use crate::rules::ini_parser::IniFile;

fn read(text: &str) -> PhobosRoofProductionAnim {
    PhobosRoofProductionAnim::from_section(IniFile::from_str(text).section_or_empty("BUILD"))
}

/// All eleven keys read independently; `none` spellings stay unresolved.
#[test]
fn every_roof_production_key_reads_on_its_own() {
    let set = read(
        "[BUILD]\n\
         RoofProductionAnim=ROOF1\n\
         RoofProductionAnimDamaged=ROOF2\n\
         RoofProductionAnimGarrisoned=ROOF3\n\
         RoofProductionAnimX=5\n\
         RoofProductionAnimY=-7\n\
         RoofProductionAnimZAdjust=12\n\
         RoofProductionAnimYSort=3\n\
         RoofProductionAnimPowered=yes\n\
         RoofProductionAnimPoweredLight=no\n\
         RoofProductionAnimPoweredEffect=yes\n\
         RoofProductionAnimPoweredSpecial=no\n",
    );
    assert_eq!(set.anim.as_deref(), Some("ROOF1"));
    assert_eq!(set.damaged.as_deref(), Some("ROOF2"));
    assert_eq!(set.garrisoned.as_deref(), Some("ROOF3"));
    assert_eq!(
        (set.x, set.y, set.z_adjust, set.y_sort),
        (Some(5), Some(-7), Some(12), Some(3))
    );
    assert_eq!(
        (
            set.powered,
            set.powered_light,
            set.powered_effect,
            set.powered_special
        ),
        (Some(true), Some(false), Some(true), Some(false))
    );
}

/// Absent keys leave every override unset; `none` clears an authored name.
#[test]
fn absent_and_none_spellings_leave_no_override() {
    let set = read("[BUILD]\nDamage=1\n");
    assert!(set.is_empty());
    assert_eq!(set.pick(true, true), None);

    let none = read("[BUILD]\nRoofProductionAnim=<none>\n");
    assert_eq!(none.anim, None);
}

/// `PickRoofProductionAnim`: Garrisoned → Damaged → base.
#[test]
fn pick_order_is_garrisoned_then_damaged_then_base() {
    let all = read(
        "[BUILD]\nRoofProductionAnim=BASE\n\
         RoofProductionAnimDamaged=DAMAGED\n\
         RoofProductionAnimGarrisoned=GARRISONED\n",
    );
    assert_eq!(all.pick(false, false), Some("BASE"));
    assert_eq!(all.pick(true, false), Some("DAMAGED"));
    assert_eq!(all.pick(true, true), Some("GARRISONED"));

    let base = read("[BUILD]\nRoofProductionAnim=BASE\n");
    assert_eq!(base.pick(true, true), Some("BASE"));

    let damaged_only = read("[BUILD]\nRoofProductionAnimDamaged=DAMAGED\n");
    assert_eq!(damaged_only.pick(false, false), None);
    assert_eq!(damaged_only.pick(true, false), Some("DAMAGED"));
}
