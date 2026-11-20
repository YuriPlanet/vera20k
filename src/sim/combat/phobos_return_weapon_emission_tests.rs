//! `ReturnWeapon=` emission: the `Damage=` scaling of
//! `Hooks.DetonateLogics.cpp:487-491` and the `BulletType` reader's wiring.

use super::return_damage;
use crate::rules::ini_parser::IniFile;
use crate::util::native_x87::NativeF64Bits;

fn mult(value: f64) -> NativeF64Bits {
    NativeF64Bits::from_bits(value.to_bits())
}

/// `Hooks.DetonateLogics.cpp:487`: with the multiplier off, `Damage=` is the
/// weapon's own.
#[test]
fn an_unapplied_multiplier_leaves_the_weapon_damage_alone() {
    assert_eq!(return_damage(100, false, mult(2.5)), 100);
    assert_eq!(return_damage(-40, false, mult(0.0)), -40);
    assert_eq!(return_damage(0, true, mult(0.0)), 0);
}

/// `:488-489`: `static_cast<int>(damage * pBulletExt->FirepowerMult)` —
/// the binary64 product truncated toward zero.
#[test]
fn an_applied_multiplier_truncates_toward_zero() {
    assert_eq!(return_damage(100, true, mult(2.5)), 250);
    assert_eq!(return_damage(100, true, mult(0.5)), 50);
    assert_eq!(
        return_damage(101, true, mult(0.5)),
        50,
        "50.5 truncates to 50"
    );
    assert_eq!(
        return_damage(-101, true, mult(0.5)),
        -50,
        "toward zero, not down"
    );
    assert_eq!(return_damage(1, true, mult(0.5)), 0);
}

/// The `int` round-trip saturates rather than wrapping, matching the
/// `SafeMultiply(int, double)` shape the reload arm uses.
#[test]
fn an_out_of_range_product_saturates() {
    assert_eq!(return_damage(i32::MAX, true, mult(2.0)), i32::MAX);
    assert_eq!(return_damage(i32::MIN, true, mult(2.0)), i32::MIN);
    assert_eq!(return_damage(i32::MAX, true, mult(f64::INFINITY)), i32::MAX);
}

/// `Ext/BulletType/Body.cpp:54-55` with the `Hooks.DetonateLogics.cpp:490`
/// fallback chain: the type's `ReturnWeapon.ApplyFirepowerMult=` wins, an
/// unset one keeps `[CombatDamage] ReturnWeapon.ApplyFirepowerMult`
/// (default `no`).
#[test]
fn the_bullet_type_reader_wires_the_two_keys() {
    let ini = IniFile::from_str(
        "[WEAP]\nDamage=10\nWarhead=WH\nProjectile=PROJ\nSpeed=20\n\
         [PROJ]\nReturnWeapon=WEAP\nReturnWeapon.ApplyFirepowerMult=yes\n\
         [CombatDamage]\nReturnWeapon.ApplyFirepowerMult=no\n",
    );
    let section = ini.section_or_empty("PROJ");
    let read = crate::rules::phobos_return_weapon::PhobosReturnWeapon::from_section(section);
    assert_eq!(read.weapon(), Some("WEAP"));
    assert_eq!(
        read.applies_firepower_mult(false),
        true,
        "the type's yes wins over the global's no"
    );

    let globals = crate::rules::phobos_globals::PhobosGlobals::from_ini(&ini);
    assert!(!globals.return_weapon_apply_firepower_mult);

    let unset = IniFile::from_str("[PROJ]\nReturnWeapon=WEAP\n");
    let read = crate::rules::phobos_return_weapon::PhobosReturnWeapon::from_section(
        unset.section_or_empty("PROJ"),
    );
    assert_eq!(
        read.applies_firepower_mult(globals.return_weapon_apply_firepower_mult),
        false,
        "an unset type key keeps the global, whose default is no"
    );
}
