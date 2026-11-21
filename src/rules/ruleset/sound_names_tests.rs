//! The sound-name table a rules set builds: retail names read back exactly,
//! a name it does not hold still travels by name, and a rebuilt set refuses
//! the ids of the set before it.

use super::{RuleSet, SoundName};
use crate::rules::ini_parser::IniFile;

const RULES: &str = "[InfantryTypes]\n0=E1\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
    [E1]\nVoiceSelect=GISelect,GIYes\nDieSound=GIDie\n";

fn fixture() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(RULES)).expect("fixture rules")
}

/// Every sound name a retail Battle type or weapon reads is held, and its id reads
/// back as the same name, case and all.
#[test]
fn retail_type_and_weapon_sounds_read_back_exactly() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let names = rules.sound_names();
    let e1 = rules.object("E1").expect("retail E1");
    assert!(!e1.voice_select.is_empty(), "retail E1 VoiceSelect");
    let weapon_reports = rules
        .weapon_types
        .iter()
        .flat_map(|weapon| weapon.report.iter())
        .map(String::as_str);
    let mut checked = 0;
    for name in rules
        .object_list
        .iter()
        .flat_map(|object| object.sound_names())
        .chain(weapon_reports)
    {
        let sound = rules.sound_name(name);
        assert!(matches!(sound, SoundName::Listed(_)), "{name} is held");
        assert_eq!(sound.resolve(Some(names)), Some(name));
        checked += 1;
    }
    assert!(checked > 1000, "retail names {checked}");
    // A name differing only in case is another name, as its String was.
    let voice = e1.voice_select[0].as_str();
    let lower = voice.to_ascii_lowercase();
    assert_ne!(voice, lower);
    assert!(!names.holds(&lower));
    assert_eq!(rules.sound_name(&lower).resolve(Some(names)), Some(&*lower));
}

/// A name the table does not hold, such as a field set after the build,
/// still plays by name, against any table or none.
#[test]
fn an_unheld_name_travels_by_name() {
    let mut rules = fixture();
    assert!(!rules.sound_names().holds("WeatherIntroLate"));
    rules.general.storm_sound = Some("WeatherIntroLate".to_string());
    let late = rules.sound_name(rules.general.storm_sound.as_deref().unwrap());
    assert_eq!(late, SoundName::Unlisted("WeatherIntroLate".into()));
    let other = fixture();
    for names in [Some(rules.sound_names()), Some(other.sound_names()), None] {
        assert_eq!(late.resolve(names), Some("WeatherIntroLate"));
    }
}

/// A rebuilt set, and a set's own table rebuilt by a binder, refuse an id of
/// the table before: an event queued across the rebuild reads nothing.
#[test]
fn a_rebuilt_table_refuses_an_old_id() {
    let mut rules = fixture();
    let old = rules.sound_name("GIDie");
    assert!(matches!(old, SoundName::Listed(_)));
    assert_eq!(old.resolve(Some(rules.sound_names())), Some("GIDie"));

    let rebuilt = fixture();
    let new = rebuilt.sound_name("GIDie");
    assert_ne!(old, new);
    assert_eq!(old.resolve(Some(rebuilt.sound_names())), None);
    assert_eq!(new.resolve(Some(rebuilt.sound_names())), Some("GIDie"));
    assert_eq!(new.resolve(Some(rules.sound_names())), None);

    rules.bind_type_sound_references(
        &IniFile::from_str(RULES),
        &crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str("")),
    );
    assert_eq!(old.resolve(Some(rules.sound_names())), None);
    assert_eq!(
        rules.sound_name("GIDie").resolve(Some(rules.sound_names())),
        Some("GIDie")
    );
}

/// The aircraft aux sounds retail emitters name (takeoff, landing, rocket
/// launch) are held, so they travel as ids too.
#[test]
fn retail_aux_sounds_are_held() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let mut checked = 0;
    for name in rules
        .object_list
        .iter()
        .flat_map(|object| [&object.aux_sound1, &object.aux_sound2])
        .flatten()
    {
        assert!(rules.sound_names().holds(name), "{name} is held");
        checked += 1;
    }
    assert!(checked > 0, "retail AuxSound1/AuxSound2");
}
