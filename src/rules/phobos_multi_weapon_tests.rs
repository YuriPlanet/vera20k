//! Phobos `MultiWeapon.*` (`Ext/TechnoType/Hooks.MultiWeapon.cpp:3-66`,
//! hook `0x7128B2`) through the production ordered TechnoType reader.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;

const RULES: &str = "[InfantryTypes]\n[VehicleTypes]\n0=MULTI\n1=PLAIN\n2=NOMULTI\n\
    [MULTI]\nStrength=200\nWeaponCount=3\nWeapon1=W0\nWeapon2=W1\nWeapon3=W2\n\
    MultiWeapon=yes\nMultiWeapon.SelectCount=3\nMultiWeapon.IsSecondary=1,9,-1\n\
    [PLAIN]\nStrength=200\nWeaponCount=3\nWeapon1=W0\nWeapon2=W1\nWeapon3=W2\n\
    MultiWeapon=yes\n\
    [NOMULTI]\nStrength=200\nWeaponCount=3\nPrimary=W0\nSecondary=W1\nWeapon3=W2\n\
    MultiWeapon=no\nMultiWeapon.SelectCount=9\nMultiWeapon.IsSecondary=2\n\
    [W0]\nDamage=1\nRange=5\nWarhead=WH\n[W1]\nDamage=1\nRange=5\nWarhead=WH\n\
    [W2]\nDamage=1\nRange=5\nWarhead=WH\n\
    [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n";

/// `MultiWeapon.SelectCount` keeps the constructor's 2 unless the switch is on
/// (`Ext/TechnoType/Hooks.MultiWeapon.cpp:35-38`); `IsSecondary` is cleared
/// when it is off (`:60-63`).
#[test]
fn multi_weapon_reads_select_count_only_while_enabled() {
    let rules = RuleSet::from_ini(&IniFile::from_str(RULES)).unwrap();
    let multi = &rules.object("MULTI").unwrap().phobos_multi_weapon;
    assert!(multi.enabled);
    assert_eq!(multi.select_count, 3);
    let plain = &rules.object("PLAIN").unwrap().phobos_multi_weapon;
    assert_eq!(plain.select_count, 2, "constructor value when unset");
    let none = &rules.object("NOMULTI").unwrap().phobos_multi_weapon;
    assert!(!none.enabled);
    assert_eq!(none.select_count, 2, "the key is not read while off");
    assert!(
        none.is_secondary.is_empty(),
        "the list is cleared while off"
    );
}

/// `MultiWeapon.IsSecondary` is sized to `WeaponCount` with false and only
/// indices `0..WeaponCount` set (`Ext/TechnoType/Hooks.MultiWeapon.cpp:41-57`);
/// `IsSecondary` (`Ext/TechnoType/Body.cpp:166-173`) then answers the list.
#[test]
fn is_secondary_sizes_to_weapon_count_and_drops_out_of_range_indices() {
    let rules = RuleSet::from_ini(&IniFile::from_str(RULES)).unwrap();
    let multi = &rules.object("MULTI").unwrap().phobos_multi_weapon;
    assert_eq!(multi.is_secondary, [false, true, false]);
    assert!(!multi.is_secondary(0, false));
    assert!(multi.is_secondary(1, false));
    assert!(!multi.is_secondary(2, false));

    // An empty list keeps the stock primary/secondary flags.
    let plain = &rules.object("PLAIN").unwrap().phobos_multi_weapon;
    assert!(plain.is_secondary.is_empty());
    assert!(!plain.is_secondary(0, false));
    assert!(plain.is_secondary(1, false));
    assert!(plain.is_secondary(2, false));

    // Gattling types ignore the list (`Body.cpp:163-164`).
    assert!(!plain.is_secondary(0, true));
    assert!(plain.is_secondary(1, true));
    assert!(!plain.is_secondary(2, true));
    assert!(plain.is_secondary(3, true));
}

/// The `ReadMultiWeapon` latch keeps the numbered weapons when the switch is
/// unchanged, and drops the stock `Primary`/`Secondary` slots when
/// `TurretCount > 0 || MultiWeapon` changes
/// (`Ext/TechnoType/Hooks.MultiWeapon.cpp:14-31`).
mod layered_rules {
    use super::IniFile;
    use super::RuleSet;
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};

    fn layered(root: &str, layer: &str) -> RuleSet {
        let mut stack = RulesLayerStack::new(IniFile::from_str(root));
        stack.push(RulesLayerKind::Scenario, IniFile::from_str(layer));
        RuleSet::from_processed_rules(&stack.process().unwrap()).unwrap()
    }

    #[test]
    fn an_unchanged_latch_keeps_the_parsed_weapons() {
        let rules = layered(
            "[InfantryTypes]\n[VehicleTypes]\n0=TANK\n[TANK]\nStrength=100\n\
             MultiWeapon=yes\nWeaponCount=3\nWeapon1=W0\nWeapon2=W1\nWeapon3=W2\n\
             [W0]\nDamage=1\n[W1]\nDamage=1\n[W2]\nDamage=1\n",
            "[TANK]\nStrength=150\n",
        );
        let object = rules.object("TANK").unwrap();
        assert!(object.phobos_multi_weapon.enabled);
        assert_eq!(object.weapon_list[0].as_deref(), Some("W0"));
        assert_eq!(object.weapon_list[1].as_deref(), Some("W1"));
        assert_eq!(object.weapon_list[2].as_deref(), Some("W2"));
    }

    #[test]
    fn switching_the_latch_on_drops_the_stock_primary_and_secondary() {
        let rules = layered(
            "[InfantryTypes]\n[VehicleTypes]\n0=TANK\n\
             [TANK]\nStrength=100\nPrimary=W0\nSecondary=W1\n\
             [W0]\nDamage=1\n[W1]\nDamage=1\n[W2]\nDamage=1\n",
            "[TANK]\nMultiWeapon=yes\nWeaponCount=3\nWeapon3=W2\n",
        );
        let object = rules.object("TANK").unwrap();
        assert_eq!(
            object.weapon_list[0], None,
            "the earlier stock Primary is cleared before the numbered read"
        );
        assert_eq!(
            object.weapon_list[1], None,
            "the earlier stock Secondary is cleared too"
        );
        assert_eq!(object.weapon_list[2].as_deref(), Some("W2"));
    }
}

/// `NoSecondaryWeaponFallback` and `.AllowAA` are independent
/// `Valueable<bool>` type keys (`Ext/TechnoType/Body.cpp:907-908`), read even
/// while the `MultiWeapon` switch is off; both constructor values are false.
#[test]
fn no_secondary_fallback_keys_read_independently_of_the_switch() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n0=FALLBACK\n1=PLAIN\n\
         [FALLBACK]\nStrength=200\nMultiWeapon=no\n\
         NoSecondaryWeaponFallback=yes\nNoSecondaryWeaponFallback.AllowAA=yes\n\
         [PLAIN]\nStrength=200\n",
    ))
    .unwrap();
    let fallback = &rules.object("FALLBACK").unwrap().phobos_multi_weapon;
    assert!(fallback.no_secondary_weapon_fallback);
    assert!(fallback.no_secondary_weapon_fallback_allow_aa);
    let plain = &rules.object("PLAIN").unwrap().phobos_multi_weapon;
    assert!(!plain.no_secondary_weapon_fallback);
    assert!(!plain.no_secondary_weapon_fallback_allow_aa);
}

/// `VoiceIFVRepair`, `VoiceWeapon%dAttack` and `VoiceEliteWeapon%dAttack`
/// (`Ext/TechnoType/Body.cpp:421-462`, `:1449`) resolve through the fixed
/// sound catalog; the slot vectors are cleared while the `ReadMultiWeapon`
/// latch is off, and an absent elite voice defaults to the normal one.
#[test]
fn multi_weapon_voice_keys_bind_per_slot_and_follow_the_latch() {
    use crate::rules::sound_ini::SoundRegistry;

    let ini = IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n0=MULTI\n1=PLAIN\n\
         [MULTI]\nStrength=200\nWeaponCount=3\nWeapon1=W0\nWeapon2=W1\nWeapon3=W1\n\
         MultiWeapon=yes\nVoiceIFVRepair=Repair\n\
         VoiceWeapon1Attack=Slot1\nVoiceWeapon2Attack=Slot2\n\
         VoiceEliteWeapon3Attack=Elite3\n\
         VoicePrimaryWeaponAttack=Primary\nVoiceSecondaryWeaponAttack=Secondary\n\
         [PLAIN]\nStrength=200\nPrimary=W0\nSecondary=W1\nVoiceWeapon1Attack=Slot1\n\
         [W0]\nDamage=1\n[W1]\nDamage=1\n",
    );
    let sounds = SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n0=Repair\n1=Slot1\n2=Slot2\n3=Elite3\n4=Primary\n5=Secondary\n",
    ));
    let mut rules = RuleSet::from_ini(&ini).unwrap();
    rules.bind_type_sound_references(&ini, &sounds);
    let multi = rules.object("MULTI").unwrap();
    assert_eq!(multi.phobos_voice_ifv_repair(), Some("Repair"));
    assert_eq!(multi.phobos_voice_weapon_attack(0, false), Some("Slot1"));
    assert_eq!(multi.phobos_voice_weapon_attack(1, false), Some("Slot2"));
    assert_eq!(multi.phobos_voice_weapon_attack(2, false), None);
    assert_eq!(multi.phobos_voice_weapon_attack(2, true), Some("Elite3"));
    assert_eq!(
        multi.phobos_voice_weapon_attack(0, true),
        Some("Slot1"),
        "an absent elite slot voice keeps the normal one"
    );
    assert_eq!(
        multi.voice_attack_override_secondary(false, false),
        Some("Primary")
    );
    assert_eq!(
        multi.voice_attack_override_secondary(true, false),
        Some("Secondary")
    );

    let plain = rules.object("PLAIN").unwrap();
    assert!(
        plain.phobos_multi_weapon.voice_weapon_attacks.is_empty(),
        "the per-slot vectors are cleared while the latch is off"
    );
    assert_eq!(plain.phobos_voice_weapon_attack(0, false), None);
}
