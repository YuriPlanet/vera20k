//! Phobos `[ShieldTypes]` reader regressions.

use super::*;
use crate::rules::ini_parser::IniFile;

#[test]
fn shield_types_read_their_section_with_native_defaults() {
    let mut catalog = ShieldTypeCatalog::default();
    catalog.apply_pass(&IniFile::from_str(
        "[ShieldTypes]\n0=TestShield\n\
         [TestShield]\nStrength=100\nInitialStrength=150\n\
         ConditionYellow=0.6\nConditionRed=0.3\nArmor=heavy\n\
         InheritArmorFromTechno=yes\nInheritArmor.Allowed=MTNK,HTNK\n\
         InheritArmor.Disallowed=MTNK\nApplyArmorMult=yes\nPowered=yes\n\
         Respawn=0.5\nRespawn.Rate=0.1\nRespawn.RestartInCombat=no\n\
         Respawn.RestartInCombatDelay=12\nRespawn.Weapon=none\n\
         SelfHealing=0.25\nSelfHealing.Rate=0.2\n\
         SelfHealing.RestartInCombatDelay=7\nSelfHealing.EnabledBy=GAPOWR\n\
         AbsorbOverDamage=yes\nAbsorbPercent=0.75\nPassPercent=0.25\n\
         ReceivedDamage.Minimum=1\nReceivedDamage.Maximum=90\n\
         Tint.Color=10,20,30\nTint.Intensity=0.5\n",
    ));

    let entry = catalog.entry("TestShield").expect("entry");
    assert_eq!(entry.strength(), 100);
    assert_eq!(entry.initial_strength(), 150);
    // ReadDouble scans an f32 and widens it, as native does.
    assert_eq!(entry.condition_yellow(0.5), f64::from(0.6_f32));
    assert_eq!(entry.condition_red(0.25), f64::from(0.3_f32));
    assert_eq!(entry.armor(), "heavy");
    assert!(entry.inherit_armor_from_techno());
    assert_eq!(entry.inherit_armor_allowed(), ["MTNK", "HTNK"]);
    assert_eq!(entry.inherit_armor_disallowed(), ["MTNK"]);
    assert!(entry.apply_armor_mult(false));
    assert!(entry.powered());
    assert_eq!(entry.respawn(), 0.5);
    assert_eq!(entry.respawn_rate(), 90);
    assert!(!entry.respawn_restart_in_combat());
    assert_eq!(entry.respawn_restart_in_combat_delay(), 12);
    assert_eq!(entry.respawn_weapon(), None);
    assert_eq!(entry.self_healing(), 0.25);
    assert_eq!(entry.self_healing_rate(), 180);
    assert_eq!(entry.self_healing_restart_in_combat_delay(), 7);
    assert!(entry.absorb_over_damage());
    assert_eq!(entry.absorb_percent(), 0.75);
    assert_eq!(entry.pass_percent(), 0.25);
    assert_eq!(entry.received_damage_minimum(), 1);
    assert_eq!(entry.received_damage_maximum(), 90);
    assert_eq!(entry.tint_color(), Some((10, 20, 30)));
    assert_eq!(entry.tint_intensity(), 0.5);
    assert!(entry.has_tint());
}

#[test]
fn shield_types_retain_unmentioned_keys_across_passes() {
    let mut catalog = ShieldTypeCatalog::default();
    catalog.apply_pass(&IniFile::from_str(
        "[ShieldTypes]\n0=KEEP\n[KEEP]\nStrength=80\nPassPercent=0.5\n",
    ));
    catalog.apply_pass(&IniFile::from_str("[KEEP]\nStrength=40\n"));

    let entry = catalog.entry("KEEP").expect("entry");
    assert_eq!(entry.strength(), 40, "a reached key replaces its value");
    assert_eq!(entry.pass_percent(), 0.5, "an omitted key retains");
    assert_eq!(
        entry.initial_strength(),
        40,
        "InitialStrength defaults to Strength"
    );
    assert_eq!(entry.respawn_restart_in_combat(), true);
    assert_eq!(entry.received_damage_minimum(), i32::MIN);
    assert_eq!(entry.received_damage_maximum(), i32::MAX);
}

#[test]
fn shield_globals_fall_back_to_the_ordinary_condition_thresholds() {
    let globals = ShieldGlobals::from_ini(&IniFile::from_str(
        "[CombatDamage]\nShieldApplyArmorMult=yes\n\
         [AudioVisual]\nConditionYellow=0.7\nConditionRed=0.4\n",
    ));
    assert!(globals.apply_armor_mult);
    assert_eq!(globals.condition_yellow, f64::from(0.7_f32));
    assert_eq!(globals.condition_red, f64::from(0.4_f32));

    let overridden = ShieldGlobals::from_ini(&IniFile::from_str(
        "[AudioVisual]\nShield.ConditionYellow=0.55\nShield.ConditionRed=0.15\n",
    ));
    assert_eq!(overridden.condition_yellow, f64::from(0.55_f32));
    assert_eq!(overridden.condition_red, f64::from(0.15_f32));
}

#[test]
fn shield_tint_visible_to_houses_reads_affected_house_flags() {
    let mut catalog = ShieldTypeCatalog::default();
    catalog.apply_pass(&IniFile::from_str(
        "[ShieldTypes]\n0=DefaultHouses\n1=TeamOnly\n2=NotOwner\n3=Bad\n\
         [DefaultHouses]\nStrength=1\n\
         [TeamOnly]\nStrength=1\nTint.VisibleToHouses= Owner , ALLY \n\
         [NotOwner]\nStrength=1\nTint.VisibleToHouses=others\n\
         [Bad]\nStrength=1\nTint.VisibleToHouses=bogus\n",
    ));

    let entry = |name: &str| catalog.entry(name).expect("entry").tint_visible_to_houses();
    // ShieldTypeClass's constructor default.
    assert_eq!(entry("DefaultHouses").bits(), 0xF);
    // Owner | Allies; spellings are case-insensitive and trimmed.
    assert_eq!(entry("TeamOnly").bits(), 0x3);
    // Others = Allies | Enemies.
    assert_eq!(entry("NotOwner").bits(), 0x6);
    // One unmatched token fails the read, so the field keeps its value.
    assert_eq!(entry("Bad").bits(), 0xF);
}

#[test]
fn shield_nullable_fields_retain_failed_reads_and_reset_to_type_defaults() {
    let mut catalog = ShieldTypeCatalog::default();
    catalog.apply_pass(&IniFile::from_str(
        "[ShieldTypes]\n0=Shield\n[Shield]\nStrength=100\nInitialStrength=$12\n\
         ConditionYellow=0.75\nConditionRed=0.25\nApplyArmorMult=yes\n\
         HitFlash.FixedSize=6\nPips.Building.Empty=5\nPips.Background=CUSTOM.SHP\n\
         Respawn.Rate=0.125\nSelfHealing.Rate=0.25\n",
    ));
    catalog.apply_pass(&IniFile::from_str(
        "[Shield]\nInitialStrength=bad\nConditionYellow=bad\nConditionRed=\n\
         ApplyArmorMult=bad\nHitFlash.FixedSize=bad\nPips.Building.Empty=bad\n\
         Respawn.Rate=bad\nSelfHealing.Rate=bad\n",
    ));
    let shield = catalog.entry("Shield").unwrap();
    assert_eq!(
        shield.initial_strength(),
        12,
        "Phobos's $ prefix is decimal"
    );
    assert_eq!(shield.condition_yellow(0.5), 0.75);
    assert_eq!(shield.condition_red(0.5), 0.25);
    assert!(shield.apply_armor_mult(false));
    assert_eq!(shield.hit_flash_fixed_size, Some(6));
    assert_eq!(shield.pips_building_empty(), Some(5));
    assert_eq!(
        (shield.respawn_rate(), shield.self_healing_rate()),
        (112, 225)
    );

    catalog.apply_pass(&IniFile::from_str(
        "[Shield]\nInitialStrength=<DeFaUlT>\nConditionYellow=none\nConditionRed=<none>\n\
         ApplyArmorMult=<default>\nHitFlash.FixedSize=<default>\n\
         Pips.Building.Empty=<default>\nPips.Background=<default>\n\
         Respawn.Rate=<default>\nSelfHealing.Rate=<default>\n",
    ));
    let shield = catalog.entry("Shield").unwrap();
    assert_eq!(shield.initial_strength(), 100);
    assert_eq!(shield.condition_yellow(0.5), 0.5);
    assert_eq!(shield.condition_red(0.5), 0.5);
    assert!(shield.apply_armor_mult(true));
    assert_eq!(shield.hit_flash_fixed_size, None);
    assert_eq!(shield.pips_building_empty(), None);
    assert_eq!(shield.pips_background(), None);
    // Each rate uses a fresh nullable scratch; clearing it retains stored frames.
    assert_eq!(
        (shield.respawn_rate(), shield.self_healing_rate()),
        (112, 225)
    );
}

#[test]
fn shield_nullable_warhead_and_global_values_keep_ordered_layer_defaults() {
    let mut ini = IniFile::from_str("");
    ini.merge_rules_projection(&IniFile::from_str(
        "[WH]\nShield.AbsorbPercent=50%\nShield.PassPercent=0.25\n\
         Shield.ReceivedDamage.Minimum=5\nShield.ReceivedDamage.Maximum=80\n\
         [AudioVisual]\nConditionYellow=0.75\nConditionRed=0.5\n\
         Shield.ConditionYellow=0.25\nShield.ConditionRed=0.125\n\
         Pips.Shield.Background=CUSTOM.SHP\nPips.Shield.Building.Empty=7\n",
    ));
    ini.merge_rules_projection(&IniFile::from_str(
        "[WH]\nShield.AbsorbPercent=bad\nShield.PassPercent=\n\
         Shield.ReceivedDamage.Minimum=bad\nShield.ReceivedDamage.Maximum=<none>\n\
         [AudioVisual]\nShield.ConditionYellow=bad\nShield.ConditionRed=<default>\n\
         Pips.Shield.Background=<default>\nPips.Shield.Building.Empty=bad\n",
    ));
    let warhead = ShieldWarheadRules::from_section(ini.section("WH").unwrap());
    assert_eq!(warhead.absorb_percent, Some(0.5));
    assert_eq!(warhead.pass_percent, Some(0.25));
    assert_eq!(warhead.received_minimum, Some(5));
    assert_eq!(warhead.received_maximum, None);
    let globals = ShieldGlobals::from_ini(&ini);
    assert_eq!(
        (globals.condition_yellow, globals.condition_red),
        (0.25, 0.5)
    );
    assert_eq!(globals.pips_background(), None);
    assert_eq!(globals.pips_building_empty(), Some(7));

    ini.merge_rules_projection(&IniFile::from_str(
        "[WH]\nShield.AbsorbPercent=<default>\nShield.PassPercent=<default>\n\
         Shield.ReceivedDamage.Minimum=<default>\n",
    ));
    let warhead = ShieldWarheadRules::from_section(ini.section("WH").unwrap());
    assert_eq!((warhead.absorb_percent, warhead.pass_percent), (None, None));
    assert_eq!(warhead.received_minimum, None);
}

#[test]
fn invalid_shield_nullable_values_preserve_unset_defaults() {
    let ini = IniFile::from_str(
        "[ShieldTypes]\n0=Shield\n[Shield]\nStrength=100\nInitialStrength=bad\n\
         ConditionYellow=bad\nApplyArmorMult=bad\n\
         [WH]\nShield.AbsorbPercent=bad\nShield.PassPercent=1e400\n\
         Shield.ReceivedDamage.Minimum=bad\n",
    );
    let mut catalog = ShieldTypeCatalog::default();
    catalog.apply_pass(&ini);
    let shield = catalog.entry("Shield").unwrap();
    assert_eq!(shield.initial_strength(), 100);
    assert_eq!(shield.condition_yellow(0.5), 0.5);
    assert!(shield.apply_armor_mult(true));
    let warhead = ShieldWarheadRules::from_section(ini.section("WH").unwrap());
    assert_eq!((warhead.absorb_percent, warhead.pass_percent), (None, None));
    assert_eq!(warhead.received_minimum, None);
}
