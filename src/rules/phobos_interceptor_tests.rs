//! Reader tests for the `Interceptor.*` TechnoType keys and the bullet-side
//! `Armor`/`Interceptable` keys.

use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_shields::AffectedHouse;
use crate::rules::ruleset::RuleSet;

fn rules(unit_keys: &str) -> RuleSet {
    let ini = IniFile::from_str(&format!(
        "[VehicleTypes]\n0=UNIT\n1=PLAIN\n[UNIT]\nStrength=100\nPrimary=GUN\n{unit_keys}\n\
         [PLAIN]\nStrength=100\n[Weapons]\n0=GUN\n[GUN]\nDamage=10\nProjectile=SHOT\n\
         [SHOT]\nStrength=50\nArmor=Heavy\nInterceptable=yes\n\
         Interceptable.DeleteOnIntercept=yes\nInterceptable.WeaponOverride=GUN\n"
    ));
    RuleSet::from_ini_with_fixed_art_for_test(&ini, &IniFile::empty()).unwrap()
}

#[test]
fn interceptor_presence_and_constructor_defaults() {
    let on = rules("Interceptor=yes");
    let settings = on
        .object("UNIT")
        .unwrap()
        .phobos_interceptor
        .as_ref()
        .expect("Interceptor=yes creates the type");
    assert_eq!(settings.targeting_delay, 1);
    assert_eq!(settings.can_target_houses, AffectedHouse::ENEMIES);
    assert_eq!(settings.guard_range, [0; 3]);
    assert_eq!(settings.minimum_guard_range, [0; 3]);
    assert_eq!(settings.guard_range_is_cylindrical, None);
    assert_eq!(settings.weapon, 0);
    assert_eq!(settings.weapon_override, None);
    assert!(!settings.weapon_replace_projectile);
    assert!(!settings.weapon_cumulative_damage);
    assert!(!settings.keep_intact);
    assert_eq!(settings.apply_firepower_mult, None);
    assert_eq!(settings.delete_on_intercept, None);

    assert!(
        on.object("PLAIN").unwrap().phobos_interceptor.is_none(),
        "a type without Interceptor= has no interceptor type"
    );
    for off in ["Interceptor=no", "Interceptor=none", "Strength=100"] {
        assert!(
            rules(off)
                .object("UNIT")
                .unwrap()
                .phobos_interceptor
                .is_none(),
            "{off:?} leaves no interceptor type"
        );
    }
}

#[test]
fn interceptor_keys_read_with_native_defaults_and_clamp() {
    let rules = rules(
        "Interceptor=yes\nInterceptor.TargetingDelay=0\nInterceptor.CanTargetHouses=all\n\
         Interceptor.GuardRange=1.5\nInterceptor.VeteranGuardRange=2\n\
         Interceptor.EliteMinimumGuardRange=0.5\nInterceptor.GuardRange.IsCylindrical=yes\n\
         Interceptor.Weapon=1\nInterceptor.WeaponOverride=GUN\n\
         Interceptor.WeaponReplaceProjectile=yes\n\
         Interceptor.WeaponCumulativeDamage=yes\nInterceptor.KeepIntact=yes\n\
         Interceptor.ApplyFirepowerMult=no\nInterceptor.DeleteOnIntercept=yes",
    );
    let settings = rules
        .object("UNIT")
        .unwrap()
        .phobos_interceptor
        .as_ref()
        .unwrap();
    assert_eq!(settings.targeting_delay, 1, "0 clamps to 1");
    assert_eq!(settings.can_target_houses, AffectedHouse::ALL);
    assert_eq!(settings.guard_range, [384, 512, 384], "cells x 256");
    assert_eq!(settings.minimum_guard_range, [0, 0, 128]);
    assert_eq!(settings.guard_range_is_cylindrical, Some(true));
    assert_eq!(settings.weapon, 1);
    assert_eq!(settings.weapon_override.as_deref(), Some("GUN"));
    assert!(settings.weapon_replace_projectile);
    assert!(settings.weapon_cumulative_damage);
    assert!(settings.keep_intact);
    assert_eq!(settings.apply_firepower_mult, Some(false));
    assert_eq!(settings.delete_on_intercept, Some(true));
}

#[test]
fn bullet_armor_strength_and_interception_keys() {
    let rules = rules("Interceptor=yes");
    let shot = rules.projectile("SHOT").unwrap();
    assert_eq!(shot.strength, 50);
    assert_eq!(shot.armor, Some(crate::rules::armor::armor_index("heavy")));
    assert_eq!(shot.interceptable, Some(true));
    assert!(shot.interceptable_delete_on_intercept);
    assert_eq!(shot.interceptable_weapon_override.as_deref(), Some("GUN"));

    // `Armor=none` clears the nullable; an unknown name leaves it unset.
    let none = IniFile::from_str("[SHOT]\nArmor=none\nInterceptable=no\n");
    let shot = crate::rules::projectile_type::ProjectileType::from_ini_section(
        "SHOT",
        none.section("SHOT").unwrap(),
        None,
    );
    assert_eq!(shot.armor, None);
    assert_eq!(shot.interceptable, Some(false));
    assert!(!shot.interceptable_delete_on_intercept);
    assert_eq!(shot.interceptable_weapon_override, None);

    let unknown = IniFile::from_str("[SHOT]\nArmor=bogus\n");
    let shot = crate::rules::projectile_type::ProjectileType::from_ini_section(
        "SHOT",
        unknown.section("SHOT").unwrap(),
        None,
    );
    assert_eq!(shot.armor, None, "an unknown armor retains the unset value");
}
