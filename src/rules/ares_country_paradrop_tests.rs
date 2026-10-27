//! Ares 3.0 `[Country] ParaDrop.*` and `Parachute.Anim=` overrides, read by
//! the common country reader `0x1001C5D0` on every pass and consulted by the
//! three country resolvers before the SideExt values.

use super::*;
use crate::rules::ruleset::RuleSet;

fn processed(ini: &str) -> RuleSet {
    let processed = RulesLayerStack::new(IniFile::from_str(ini))
        .process()
        .expect("country paradrop rules parse");
    RuleSet::from_processed_rules(&processed).expect("country paradrop projection")
}

const REGISTRY: &str = "\
[InfantryTypes]\n0=ENG\n\
[VehicleTypes]\n0=MTANK\n\
[AircraftTypes]\n0=CARGO\n1=PLANEZ\n\
[Animations]\n0=PARA1\n";

#[test]
fn ares_country_paradrop_overrides_resolve_by_identity() {
    let rules = processed(&format!(
        "{REGISTRY}[Countries]\n0=Greece\n\
         [Greece]\n\
         Parachute.Anim=PARA1\n\
         ParaDrop.Aircraft=PLANEZ\n\
         ParaDrop.Types=MTANK,ENG,NOTHING\n\
         ParaDrop.Num=2,3\n"
    ));
    assert_eq!(rules.country_parachute_anim("Greece"), Some("PARA1"));
    assert_eq!(rules.country_paradrop_aircraft("Greece"), Some("PLANEZ"));
    let expected: Option<(&[String], &[i32])> = Some((
        &[String::from("MTANK"), String::from("ENG")][..],
        &[2, 3][..],
    ));
    // `remove_non_paradroppables` drops the unregistered third name.
    assert_eq!(rules.country_paradrop_lists("Greece"), expected);
}

#[test]
fn ares_country_paradrop_absence_defers_and_types_without_counts_stay_country_lists() {
    let unset = processed(&format!("{REGISTRY}[Countries]\n0=Greece\n[Greece]\n"));
    assert_eq!(unset.country_paradrop_lists("Greece"), None);
    assert_eq!(unset.country_paradrop_aircraft("Greece"), None);
    assert_eq!(unset.country_parachute_anim("Greece"), None);

    // `GetParadropContent @ 0x1001BC20` still answers the country's lists for
    // a non-empty type list with no counts; the sender's length check then
    // sends nothing.
    let no_counts = processed(&format!(
        "{REGISTRY}[Countries]\n0=Greece\n[Greece]\nParaDrop.Types=ENG\n"
    ));
    let expected: Option<(&[String], &[i32])> = Some((&[String::from("ENG")][..], &[][..]));
    assert_eq!(no_counts.country_paradrop_lists("Greece"), expected);
}

/// `0x1001C5D0` runs the parent copy (`0x1001BDC0`) before these reads, so a
/// child's own keys override what it inherits. An unknown name and an empty
/// value keep the inherited field, and `none` clears it (`0x1000C9C0`,
/// `0x1001CC10`, `0x1000EDD0`, `0x1000ED00`).
#[test]
fn ares_country_child_paradrop_keys_override_the_parent_copy() {
    let mut countries = IniSection::new("Countries".into());
    countries.set("0", "Greece");
    countries.set("1", "Rome");
    let mut greece = IniSection::new("Greece".into());
    for (key, value) in [
        ("Parachute.Anim", "PARA1"),
        ("ParaDrop.Aircraft", "PLANEZ"),
        ("ParaDrop.Types", "MTANK"),
        ("ParaDrop.Num", "4"),
    ] {
        greece.set(key, value);
    }
    let mut rome = IniSection::new("Rome".into());
    for (key, value) in [
        ("ParentCountry", "Greece"),
        ("Parachute.Anim", "NOPE"),
        ("ParaDrop.Aircraft", "none"),
        ("ParaDrop.Types", "ENG"),
        ("ParaDrop.Num", ""),
    ] {
        rome.set(key, value);
    }
    let registry = IniFile::from_str(REGISTRY);
    let mut sections: Vec<IniSection> = registry
        .section_names()
        .iter()
        .map(|name| registry.section(name).unwrap().clone())
        .collect();
    sections.extend([countries, greece, rome]);
    let processed = RulesLayerStack::new(IniFile::from_sections_for_test(sections))
        .process()
        .expect("country paradrop rules parse");
    let rules = RuleSet::from_processed_rules(&processed).expect("country paradrop projection");
    assert_eq!(rules.country_parachute_anim("Rome"), Some("PARA1"));
    assert_eq!(rules.country_paradrop_aircraft("Rome"), None);
    let expected: Option<(&[String], &[i32])> = Some((&[String::from("ENG")][..], &[4][..]));
    assert_eq!(rules.country_paradrop_lists("Rome"), expected);
}

/// `0x1001C530` calls the common reader for every INI, not only the Rules
/// file, so a later (map) pass sets the country's plane.
#[test]
fn ares_country_paradrop_is_read_on_later_passes() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(&format!(
        "{REGISTRY}[Countries]\n0=Greece\n[Greece]\n"
    )));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[Greece]\nParaDrop.Aircraft=PLANEZ\n"),
    );
    let processed = layers.process().expect("country paradrop map pass");
    let rules = RuleSet::from_processed_rules(&processed).expect("country paradrop projection");
    assert_eq!(rules.country_paradrop_aircraft("Greece"), Some("PLANEZ"));
}
