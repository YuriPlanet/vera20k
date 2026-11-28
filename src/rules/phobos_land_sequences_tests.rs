//! `OnlyUseLandSequences=` on an `InfantryType`'s art section.

use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_land_sequences::PhobosLandSequences;

fn read(keys: &str) -> PhobosLandSequences {
    PhobosLandSequences::from_section(
        IniFile::from_str(&format!("[Q]\n{keys}")).section_or_empty("Q"),
    )
}

/// `Ext/InfantryType/Body.h:44`: the key constructs unset and defers to
/// `[General] OnlyUseLandSequences` (default `no`).
#[test]
fn an_unset_key_defers_to_the_general_default() {
    let unset = read("");
    assert_eq!(unset.only_use_land_sequences, None);
    assert!(!unset.only_land(false), "[General] defaults to no");
    assert!(unset.only_land(true), "a set global applies");
}

/// `Ext/InfantryType/Body.cpp:32`: the type's `yes`/`no` outranks the global.
#[test]
fn the_types_value_outranks_the_global() {
    let yes = read("OnlyUseLandSequences=yes\n");
    assert!(yes.only_land(false));
    assert!(yes.only_land(true));

    let no = read("OnlyUseLandSequences=no\n");
    assert!(
        !no.only_land(true),
        "the type's no wins over the global's yes"
    );
    assert!(!no.only_land(false));
}

/// `Nullable<bool>::Read` (`Utilities/TemplateDef.h:1833-1849`): `<default>`,
/// `none` and `<none>` reset the key back to the global.
#[test]
fn a_default_or_none_read_clears_the_key() {
    for key in [
        "OnlyUseLandSequences=<default>",
        "OnlyUseLandSequences=none",
        "OnlyUseLandSequences=<none>",
    ] {
        let read = read(&format!("{key}\n"));
        assert_eq!(read.only_use_land_sequences, None, "{key} resets");
        assert!(!read.only_land(false), "{key} falls back to the global");
    }
    assert_eq!(
        read("OnlyUseLandSequences=\n").only_use_land_sequences,
        None
    );
}

/// `Ext/InfantryType/Body.cpp:33`, `Rules/Body.h:1018`: the art key defaults
/// unset and the global constructs yes; a type's no disables the wet-attack
/// remap for SecondaryFire/SecondaryProne.
#[test]
fn secondary_fire_land_only_defaults_on_and_the_type_can_disable_it() {
    let unset = read("");
    assert_eq!(unset.secondary_fire_sequence_land_only, None);
    assert!(
        unset.secondary_fire_land_only(true),
        "the global defaults yes"
    );
    assert!(
        !unset.secondary_fire_land_only(false),
        "a set global applies"
    );

    let no = read("SecondaryFireSequenceLandOnly=no\n");
    assert!(!no.secondary_fire_land_only(true), "the type's no wins");
    let yes = read("SecondaryFireSequenceLandOnly=yes\n");
    assert!(yes.secondary_fire_land_only(false), "the type's yes wins");
    assert_eq!(
        read("SecondaryFireSequenceLandOnly=none\n").secondary_fire_sequence_land_only,
        None,
        "none resets to the global"
    );
}
