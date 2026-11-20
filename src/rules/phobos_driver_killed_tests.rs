//! `DriverKilled.*` reads (`Ext/TechnoType/Body.cpp:969-970`) and the
//! `[CombatDamage]` fallbacks (`Ext/Rules/Body.cpp:533-534`).
use super::PhobosDriverKilled;
use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_globals::PhobosGlobals;

fn read(text: &str) -> PhobosDriverKilled {
    PhobosDriverKilled::from_section(IniFile::from_str(text).section_or_empty("Q"))
}

#[test]
fn driver_killed_keys_read_with_their_global_fallbacks() {
    let defaults = PhobosDriverKilled::default();
    assert_eq!(read("[Q]\n"), defaults);
    assert_eq!(defaults.kept_passengers, None);
    assert_eq!(defaults.kill_passengers, None);

    let set = read(
        "[Q]\n\
         DriverKilled.KeptPassengers=yes\n\
         DriverKilled.KillPassengers=no\n",
    );
    assert_eq!(set.kept_passengers, Some(true));
    assert_eq!(set.kill_passengers, Some(false));

    // `Nullable<bool>` clears on an absent key or a clearing token.
    let cleared = read("[Q]\nDriverKilled.KeptPassengers=<none>\n");
    assert_eq!(cleared.kept_passengers, None);

    let globals = PhobosGlobals::default();
    assert!(!globals.driver_killed_kept_passengers);
    assert!(!globals.driver_killed_kill_passengers);
    let globals = PhobosGlobals::from_ini(&IniFile::from_str(
        "[CombatDamage]\n\
         DriverKilled.KeptPassengers=yes\n\
         DriverKilled.KillPassengers=yes\n",
    ));
    assert!(globals.driver_killed_kept_passengers);
    assert!(globals.driver_killed_kill_passengers);

    // `Nullable<T>::Get` returns the type value when set, else the global.
    let unset = PhobosDriverKilled::default();
    assert!(
        unset.kept_passengers(globals.driver_killed_kept_passengers),
        "an unset type key falls back to the global"
    );
    assert!(!unset.kept_passengers(false));
    assert!(set.kept_passengers(false), "a set type value wins");
    assert!(
        unset.kill_passengers(globals.driver_killed_kill_passengers),
        "an unset type key falls back to the global"
    );
    assert!(!unset.kill_passengers(false));
    assert!(!set.kill_passengers(true), "a set type value wins");
}
