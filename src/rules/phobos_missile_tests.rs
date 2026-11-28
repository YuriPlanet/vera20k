//! The `Missile.*` reads (`Ext/AircraftType/Body.cpp:42-44`,
//! `AircraftTypeExt::Initialize` at `Body.cpp:12`).
use super::{PhobosMissileRules, takeoff_settings};
use crate::rules::ini_parser::IniFile;

#[test]
fn defaults_match_initialize() {
    let set = PhobosMissileRules::default();
    assert!(!set.cruise);
    assert_eq!(set.takeoff_separation, 24);
    assert_eq!(set.takeoff_anim.as_deref(), Some("V3TAKOFF"));
    assert!(!set.cruise);
}

#[test]
fn reads_the_three_keys_and_the_anim_clearing_tokens() {
    let ini = IniFile::from_str(
        "[A]\nMissile.Cruise=yes\nMissile.TakeOffSeparation=5\nMissile.TakeOffAnim=MYTAKE\n\
         [B]\nMissile.TakeOffAnim=<none>\n\
         [C]\nMissile.TakeOffAnim=\n",
    );
    let set = PhobosMissileRules::from_section(ini.section_or_empty("A"));
    assert!(set.cruise);
    assert_eq!(set.takeoff_separation, 5);
    assert_eq!(set.takeoff_anim.as_deref(), Some("MYTAKE"));

    assert_eq!(
        PhobosMissileRules::from_section(ini.section_or_empty("B")).takeoff_anim,
        None,
        "<none> clears the Valueable<AnimTypeClass*>"
    );
    assert_eq!(
        PhobosMissileRules::from_section(ini.section_or_empty("C")).takeoff_anim,
        Some("V3TAKOFF".to_owned()),
        "a blank read keeps the current value"
    );
}

#[test]
fn takeoff_settings_prefer_the_type_keys() {
    let typed = PhobosMissileRules {
        cruise: true,
        takeoff_separation: 7,
        takeoff_anim: Some("TYPE".to_owned()),
    };
    assert_eq!(
        takeoff_settings(Some(&typed), Some("BLOCK")),
        (Some("TYPE"), 7)
    );
    let cleared = PhobosMissileRules {
        takeoff_anim: None,
        ..PhobosMissileRules::default()
    };
    assert_eq!(
        takeoff_settings(Some(&cleared), Some("BLOCK")),
        (None, 24),
        "a cleared type anim means no takeoff puff"
    );
    assert_eq!(
        takeoff_settings(None, Some("BLOCK")),
        (Some("BLOCK"), 24),
        "without type settings the block anim and stock 24 remain"
    );
}
