//! `DetonateOnAllMapObjects.*` reads (`Ext/WarheadType/Body.cpp:304-319`).
use super::DetonateOnAllMapObjectsRules;
use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_crit::AffectedTarget;
use crate::rules::phobos_shields::AffectedHouse;

fn read(text: &str) -> DetonateOnAllMapObjectsRules {
    DetonateOnAllMapObjectsRules::from_section(IniFile::from_str(text).section_or_empty("Q"))
}

#[test]
fn detonate_on_all_map_objects_defaults_and_reads() {
    let defaults = DetonateOnAllMapObjectsRules::default();
    assert_eq!(read("[Q]\n"), defaults);
    assert!(!defaults.enabled);
    assert!(defaults.full, "Full constructs yes");
    assert!(!defaults.require_verses);
    assert_eq!(defaults.affects_target, AffectedTarget::NONE);
    assert_eq!(defaults.affects_house, AffectedHouse::NONE);
    assert!(defaults.affect_types.is_empty() && defaults.ignore_types.is_empty());
    assert!(
        !defaults.claims_detonation(),
        "None on either flag is inert"
    );

    let set = read(
        "[Q]\n\
         DetonateOnAllMapObjects=yes\n\
         DetonateOnAllMapObjects.Full=no\n\
         DetonateOnAllMapObjects.RequireVerses=yes\n\
         DetonateOnAllMapObjects.AffectsTarget=units,buildings\n\
         DetonateOnAllMapObjects.AffectsHouse=enemies\n\
         DetonateOnAllMapObjects.AffectTypes=GT\n\
         DetonateOnAllMapObjects.IgnoreTypes=EX\n",
    );
    assert!(set.enabled);
    assert!(!set.full);
    assert!(set.require_verses);
    assert_eq!(
        set.affects_target.bits(),
        AffectedTarget::UNIT.bits() | AffectedTarget::BUILDING.bits()
    );
    assert_eq!(set.affects_house, AffectedHouse::ENEMIES);
    assert_eq!(set.affect_types, ["GT"]);
    assert_eq!(set.ignore_types, ["EX"]);
    assert!(set.claims_detonation());
    assert!(set.affects(AffectedTarget::UNIT));
    assert!(!set.affects(AffectedTarget::AIRCRAFT));
}

#[test]
fn deprecated_spellings_lose_to_the_real_keys() {
    let rules = read(
        "[Q]\nDetonateOnAllMapObjects=yes\n\
         DetonateOnAllMapObjects.AffectTargets=infantry\n\
         DetonateOnAllMapObjects.AffectsTarget=aircraft\n\
         DetonateOnAllMapObjects.AffectHouses=owner\n\
         DetonateOnAllMapObjects.AffectsHouse=allies\n",
    );
    assert_eq!(rules.affects_target, AffectedTarget::AIRCRAFT);
    assert_eq!(rules.affects_house, AffectedHouse::ALLIES);
    // Only the deprecated key present still reads.
    let deprecated = read(
        "[Q]\nDetonateOnAllMapObjects=yes\n\
         DetonateOnAllMapObjects.AffectTargets=infantry\n\
         DetonateOnAllMapObjects.AffectHouses=owner\n",
    );
    assert_eq!(deprecated.affects_target, AffectedTarget::INFANTRY);
    assert_eq!(deprecated.affects_house, AffectedHouse::OWNER);
}
