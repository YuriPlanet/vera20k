//! The `ForceWeapon.*` / `ForceAAWeapon.*` reads
//! (`Ext/TechnoType/Body.cpp:905-936`, constructors `Body.h:604-624`).
use super::PhobosForceWeapon;
use crate::rules::ini_parser::IniFile;

#[test]
fn defaults_match_the_constructor_and_gate_every_arm() {
    let set = PhobosForceWeapon::default();
    assert_eq!(
        (
            set.naval_decloaked,
            set.cloaked,
            set.disguised,
            set.under_emp
        ),
        (-1, -1, -1, -1)
    );
    assert_eq!(
        (set.buildings, set.defenses, set.infantry, set.naval_units),
        (-1, -1, -1, -1)
    );
    assert_eq!(
        (set.units, set.aircraft, set.aa_infantry, set.aa_units),
        (-1, -1, -1, -1)
    );
    assert_eq!(set.aa_aircraft, -1);
    assert!(set.in_range.is_empty() && set.aa_in_range.is_empty());
    assert_eq!(
        (set.in_range_techno_only, set.in_range_apply_range_modifiers),
        (None, None),
        "Nullables keep the global fallback"
    );
    assert!(!set.any(), "no configured arm skips forced selection");
}

#[test]
fn reads_state_class_and_ordered_in_range_arms() {
    let set = PhobosForceWeapon::from_section(
        IniFile::from_str(
            "[TYPE]\nForceWeapon.Naval.Decloaked=1\nForceWeapon.Cloaked=1\n\
             ForceWeapon.Disguised=2\nForceWeapon.UnderEMP=0\n\
             ForceWeapon.Buildings=2\nForceWeapon.Defenses=1\n\
             ForceWeapon.Infantry=1\nForceWeapon.Naval.Units=2\n\
             ForceWeapon.Units=1\nForceWeapon.Aircraft=1\n\
             ForceAAWeapon.Infantry=1\nForceAAWeapon.Units=2\nForceAAWeapon.Aircraft=1\n\
             ForceWeapon.InRange=1,-1,2\nForceWeapon.InRange.Overrides=1.5,-1\n\
             ForceWeapon.InRange.TechnoOnly=no\n\
             ForceAAWeapon.InRange=1,3\nForceAAWeapon.InRange.Overrides=0.5\n\
             ForceAAWeapon.InRange.ApplyRangeModifiers=yes\n",
        )
        .section_or_empty("TYPE"),
    );
    assert_eq!(set.naval_decloaked, 1);
    assert_eq!(set.cloaked, 1);
    assert_eq!(set.disguised, 2);
    assert_eq!(set.under_emp, 0);
    assert_eq!((set.buildings, set.defenses), (2, 1));
    assert_eq!((set.infantry, set.naval_units, set.units), (1, 2, 1));
    assert_eq!((set.aircraft, set.aa_infantry, set.aa_units), (1, 1, 2));
    assert_eq!(set.aa_aircraft, 1);
    assert_eq!(set.in_range, vec![1, -1, 2]);
    assert_eq!(set.in_range_overrides, vec![1.5, -1.0]);
    assert_eq!(set.in_range_techno_only, Some(false));
    assert_eq!(set.aa_in_range, vec![1, 3]);
    assert_eq!(set.aa_in_range_overrides, vec![0.5]);
    assert_eq!(set.aa_in_range_apply_range_modifiers, Some(true));
    assert!(set.any());

    // A present vector replaces the list; a token that does not convert is
    // skipped, and `<default>` clears a Nullable.
    let replaced = PhobosForceWeapon::from_section(
        IniFile::from_str(
            "[TYPE]\nForceWeapon.InRange=1,bogus,3\nForceWeapon.InRange.TechnoOnly=<default>\n",
        )
        .section_or_empty("TYPE"),
    );
    assert_eq!(replaced.in_range, vec![1, 3]);
    assert_eq!(replaced.in_range_techno_only, None);
    assert!(
        replaced.any(),
        "a non-empty in-range list arms the forced-selection check"
    );

    // `ValueableVector<double>` tokens go through the Phobos Parser<double>
    // scan: a percent scales at 0.01f and an unconvertible token is skipped.
    let percent = PhobosForceWeapon::from_section(
        IniFile::from_str(
            "[TYPE]\nForceWeapon.InRange=1,2\nForceWeapon.InRange.Overrides=50%,bogus\n",
        )
        .section_or_empty("TYPE"),
    );
    assert_eq!(percent.in_range_overrides, vec![0.5]);
}
