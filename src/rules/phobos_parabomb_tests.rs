//! `Parachuted=` / `Parachuted.FallRate=` / `Parachuted.MaxFallRate=` /
//! `BombParachute=` on a `BulletType`.

use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_parabomb::PhobosParabomb;

fn read(keys: &str) -> PhobosParabomb {
    PhobosParabomb::from_section(IniFile::from_str(&format!("[Q]\n{keys}")).section_or_empty("Q"))
}

/// `Ext/BulletType/Body.h:145-148`: the four keys construct at their native
/// defaults — `Parachuted` off, a fall rate of 1, and both pointers null.
#[test]
fn the_four_keys_construct_at_their_native_defaults() {
    let defaults = PhobosParabomb::default();
    assert_eq!(read(""), defaults);
    assert!(!defaults.parachuted);
    assert_eq!(defaults.fall_rate, 1);
    assert_eq!(defaults.max_fall_rate, None);
    assert_eq!(defaults.bomb_parachute, None);
}

/// `Ext/BulletType/Body.cpp:79-82`: each key reads on its own.
#[test]
fn every_key_reads_on_its_own() {
    let set = read(
        "Parachuted=yes\n\
         Parachuted.FallRate=3\n\
         Parachuted.MaxFallRate=-20\n\
         BombParachute=PARABOMB\n",
    );
    assert!(set.parachuted);
    assert_eq!(set.fall_rate, 3);
    assert_eq!(set.max_fall_rate, Some(-20));
    assert_eq!(set.bomb_parachute.as_deref(), Some("PARABOMB"));

    let flag_only = read("Parachuted=yes\n");
    assert!(flag_only.parachuted);
    assert_eq!(flag_only.fall_rate, 1);
    assert_eq!(flag_only.max_fall_rate, None);
    assert_eq!(flag_only.bomb_parachute, None);

    let rate_only = read("Parachuted.FallRate=7\n");
    assert!(!rate_only.parachuted);
    assert_eq!(rate_only.fall_rate, 7);

    let canopy_only = read("BombParachute=CHUTE\n");
    assert!(!canopy_only.parachuted);
    assert_eq!(canopy_only.bomb_parachute.as_deref(), Some("CHUTE"));
}

/// `Ext/Bullet/Hooks.cpp:131`: an unset `Parachuted.MaxFallRate=` keeps
/// `[General] ParachuteMaxFallRate`.
#[test]
fn an_unset_max_fall_rate_falls_back_to_the_general_key() {
    let unset = PhobosParabomb::default();
    assert_eq!(unset.max_fall_rate(-100), -100, "the global is the cap");
    assert_eq!(unset.max_fall_rate(0), 0);

    let set = read("Parachuted.MaxFallRate=-7\n");
    assert_eq!(set.max_fall_rate(-100), -7, "the type's value wins");
    assert_eq!(set.max_fall_rate(0), -7);
}

/// `Nullable<T>::Read` (`Utilities/TemplateDef.h:1833-1849`): `<default>`
/// resets, and so does the type-pointer sentinel `none`/`<none>`. A blank
/// read never reaches the reader (`ReadString` answers 0) and therefore keeps
/// whatever the caller had — here the unset `None` of a fresh parse.
#[test]
fn a_default_read_clears_the_nullables() {
    for key in [
        "Parachuted.MaxFallRate=<default>",
        "Parachuted.MaxFallRate=",
    ] {
        assert_eq!(
            read(&format!("{key}\n")).max_fall_rate,
            None,
            "{key} leaves the key unset"
        );
    }
    assert_eq!(read("BombParachute=<none>\n").bomb_parachute, None);
    assert_eq!(read("BombParachute=none\n").bomb_parachute, None);
    assert_eq!(read("BombParachute=\n").bomb_parachute, None);
}

/// `Parachuted.FallRate=` is a plain `Valueable<int>` with no reset token; a
/// blank read keeps whatever the caller had, which the reader passes in as
/// the default of 1.
#[test]
fn a_blank_fall_rate_keeps_the_default_step() {
    assert_eq!(read("Parachuted.FallRate=\n").fall_rate, 1);
    assert_eq!(read("Parachuted.FallRate=0\n").fall_rate, 0);
}
