//! `KillWeapon.*` and `SuppressKillWeapons.*` reads.
use super::{PhobosKillWeaponRules, PhobosKillWeaponSuppress};
use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_crit::AffectedTarget;
use crate::rules::phobos_shields::AffectedHouse;

#[test]
fn kill_weapon_defaults_allow_every_house_and_target() {
    let warhead =
        PhobosKillWeaponRules::from_section(IniFile::from_str("[WH]\n").section_or_empty("WH"));
    assert_eq!(warhead, PhobosKillWeaponRules::default());
    assert_eq!(warhead.kill_weapon_affects_house, AffectedHouse::ALL);
    assert_eq!(warhead.kill_weapon_affects_target, AffectedTarget::ALL);
    let techno = PhobosKillWeaponSuppress::from_section(
        IniFile::from_str("[UNIT]\n").section_or_empty("UNIT"),
    );
    assert!(!techno.suppress);
    assert!(techno.types.is_empty());
}

#[test]
fn kill_weapon_reads_names_houses_and_targets() {
    let warhead = PhobosKillWeaponRules::from_section(
        IniFile::from_str(
            "[WH]\nKillWeapon=KILL\nKillWeapon.OnFirer=ONFIRER\n\
             KillWeapon.AffectsHouse=owner,allies\n\
             KillWeapon.OnFirer.AffectsHouse=enemies\n\
             KillWeapon.AffectsTarget=infantry,units\n\
             KillWeapon.OnFirer.AffectsTarget=buildings\n",
        )
        .section_or_empty("WH"),
    );
    assert_eq!(warhead.kill_weapon.as_deref(), Some("KILL"));
    assert_eq!(warhead.kill_weapon_on_firer.as_deref(), Some("ONFIRER"));
    assert!(
        warhead
            .kill_weapon_affects_house
            .contains(AffectedHouse::OWNER)
            && warhead
                .kill_weapon_affects_house
                .contains(AffectedHouse::ALLIES)
            && !warhead
                .kill_weapon_affects_house
                .contains(AffectedHouse::ENEMIES)
    );
    assert_eq!(
        warhead.kill_weapon_on_firer_affects_house,
        AffectedHouse::ENEMIES
    );
    assert!(
        warhead
            .kill_weapon_affects_target
            .contains(AffectedTarget::INFANTRY)
    );
    assert!(
        warhead
            .kill_weapon_affects_target
            .contains(AffectedTarget::UNIT)
    );
    assert!(
        !warhead
            .kill_weapon_affects_target
            .contains(AffectedTarget::BUILDING)
    );
    assert!(
        warhead
            .kill_weapon_on_firer_affects_target
            .contains(AffectedTarget::BUILDING)
    );
    assert!(
        !warhead
            .kill_weapon_on_firer_affects_target
            .contains(AffectedTarget::UNIT)
    );
}

#[test]
fn deprecated_aliases_are_read_before_the_canonical_keys() {
    let warhead = PhobosKillWeaponRules::from_section(
        IniFile::from_str(
            "[WH]\nKillWeapon.AffectsHouses=owner\nKillWeapon.AffectsHouse=enemies\n\
             KillWeapon.OnFirer.Affects=infantry\nKillWeapon.OnFirer.AffectsTarget=buildings\n",
        )
        .section_or_empty("WH"),
    );
    assert_eq!(warhead.kill_weapon_affects_house, AffectedHouse::ENEMIES);
    assert!(
        warhead
            .kill_weapon_on_firer_affects_target
            .contains(AffectedTarget::BUILDING)
    );
}

#[test]
fn suppress_kill_weapons_drops_unlisted_or_everything() {
    let read = |text: &str| {
        PhobosKillWeaponSuppress::from_section(IniFile::from_str(text).section_or_empty("UNIT"))
    };
    assert!(!read("[UNIT]\n").suppresses("KILL"));
    assert!(read("[UNIT]\nSuppressKillWeapons=yes\n").suppresses("KILL"));
    let filtered = read("[UNIT]\nSuppressKillWeapons=yes\nSuppressKillWeapons.Types=KILL\n");
    assert!(
        filtered.suppresses("kill"),
        "names compare case-insensitively"
    );
    assert!(
        !filtered.suppresses("OTHER"),
        "an unlisted weapon still fires"
    );
}
