//! `ReturnWeapon=` / `ReturnWeapon.ApplyFirepowerMult=` on a `BulletType`.

use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_return_weapon::PhobosReturnWeapon;

fn read(keys: &str) -> PhobosReturnWeapon {
    PhobosReturnWeapon::from_section(
        IniFile::from_str(&format!("[Q]\n{keys}")).section_or_empty("Q"),
    )
}

/// `Ext/BulletType/Body.h:121-122`: both keys construct unset.
#[test]
fn the_pair_constructs_unset() {
    let defaults = PhobosReturnWeapon::default();
    assert_eq!(read(""), defaults);
    assert_eq!(defaults.return_weapon, None);
    assert_eq!(defaults.apply_firepower_mult, None);
    assert_eq!(defaults.weapon(), None);
}

/// `Ext/BulletType/Body.cpp:54-55`: the weapon name is kept as written and the
/// multiplier override reads on its own.
#[test]
fn the_weapon_and_its_multiplier_read_independently() {
    let set = read("ReturnWeapon=RETALIATE\nReturnWeapon.ApplyFirepowerMult=yes\n");
    assert_eq!(set.weapon(), Some("RETALIATE"));
    assert_eq!(set.apply_firepower_mult, Some(true));

    let weapon_only = read("ReturnWeapon=RETALIATE\n");
    assert_eq!(weapon_only.weapon(), Some("RETALIATE"));
    assert_eq!(weapon_only.apply_firepower_mult, None);
    assert_eq!(read("ApplyFirepowerMult=no\n").weapon(), None);
}

/// `Hooks.DetonateLogics.cpp:490`: the type's value wins, and an unset one
/// keeps `[CombatDamage] ReturnWeapon.ApplyFirepowerMult`, whose default is
/// `no`.
#[test]
fn the_global_applies_only_while_the_type_stays_unset() {
    let unset = PhobosReturnWeapon::default();
    assert!(!unset.applies_firepower_mult(false));
    assert!(unset.applies_firepower_mult(true));

    let no = read("ReturnWeapon.ApplyFirepowerMult=no\n");
    assert!(!no.applies_firepower_mult(true), "the type's no wins");

    let yes = read("ReturnWeapon.ApplyFirepowerMult=yes\n");
    assert!(yes.applies_firepower_mult(false), "the type's yes wins");
}

/// `Nullable<bool>::Read` (`Utilities/TemplateDef.h:1833-1849`) and the
/// `<none>` token of a type pointer: both clear the key.
#[test]
fn a_default_read_clears_the_multiplier_and_none_clears_the_weapon() {
    for key in [
        "ReturnWeapon.ApplyFirepowerMult=<default>",
        "ReturnWeapon.ApplyFirepowerMult=",
    ] {
        assert_eq!(
            read(&format!("{key}\n")).apply_firepower_mult,
            None,
            "{key} resets"
        );
    }
    assert_eq!(read("ReturnWeapon=<none>\n").weapon(), None);
}
