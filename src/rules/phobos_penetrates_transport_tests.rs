//! The `PenetratesTransport.*` reads (`Ext/WarheadType/Body.cpp:437-442`,
//! `Ext/TechnoType/Body.cpp:1174-1177`).
use super::{PenetratesTransportTechnoRules, PenetratesTransportWarheadRules};
use crate::rules::ini_parser::IniFile;

#[test]
fn warhead_defaults_match_the_constructor() {
    let set = PenetratesTransportWarheadRules::default();
    assert_eq!(set.level, 0);
    assert_eq!(set.pass_through, 1.0);
    assert_eq!(set.fatal_rate, 0.0);
    assert_eq!(set.damage_multiplier, 1.0);
    assert!(!set.damage_all);
    assert_eq!(set.clean_sound, None);
}

#[test]
fn warhead_reads_the_six_keys() {
    let set = PenetratesTransportWarheadRules::from_section(
        IniFile::from_str(
            "[WH]\nPenetratesTransport.Level=5\nPenetratesTransport.PassThrough=0.5\n\
             PenetratesTransport.FatalRate=0.25\nPenetratesTransport.DamageMultiplier=2\n\
             PenetratesTransport.DamageAll=yes\nPenetratesTransport.CleanSound=CleanUp\n",
        )
        .section_or_empty("WH"),
    );
    assert_eq!(set.level, 5);
    assert_eq!(set.pass_through, 0.5);
    assert_eq!(set.fatal_rate, 0.25);
    assert_eq!(set.damage_multiplier, 2.0);
    assert!(set.damage_all);
    assert_eq!(set.clean_sound.as_deref(), Some("CleanUp"));

    // `<none>` clears the ValueableIdx sound to the constructor's -1.
    let cleared = PenetratesTransportWarheadRules::from_section(
        IniFile::from_str("[WH]\nPenetratesTransport.CleanSound=<none>\n").section_or_empty("WH"),
    );
    assert_eq!(cleared.clean_sound, None);
}

#[test]
fn techno_defaults_reads_and_global_level_fallback() {
    let set = PenetratesTransportTechnoRules::default();
    assert_eq!(set.level, None);
    assert_eq!(set.level(10), 10);
    assert_eq!(set.pass_through_multiplier, 1.0);
    assert_eq!(set.fatal_rate_multiplier, 1.0);
    assert_eq!(set.damage_multiplier, 1.0);

    let ini = IniFile::from_str(
        "[TRANS]\nPenetratesTransport.Level=7\nPenetratesTransport.PassThroughMultiplier=0.5\n\
         PenetratesTransport.FatalRateMultiplier=2\nPenetratesTransport.DamageMultiplier=3\n\
         [CLEARED]\nPenetratesTransport.Level=<default>\n",
    );
    let set = PenetratesTransportTechnoRules::from_section(ini.section_or_empty("TRANS"));
    assert_eq!(set.level(10), 7);
    assert_eq!(set.pass_through_multiplier, 0.5);
    assert_eq!(set.fatal_rate_multiplier, 2.0);
    assert_eq!(set.damage_multiplier, 3.0);
    let cleared = PenetratesTransportTechnoRules::from_section(ini.section_or_empty("CLEARED"));
    assert_eq!(cleared.level(10), 10, "<default> restores the global");
}
