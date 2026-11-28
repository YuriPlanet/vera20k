//! `ExitThroughRoof=` on a `TechnoType`.

use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_exit_through_roof::PhobosExitThroughRoof;

fn read(keys: &str) -> PhobosExitThroughRoof {
    PhobosExitThroughRoof::from_section(
        IniFile::from_str(&format!("[Q]\n{keys}")).section_or_empty("Q"),
    )
}

/// `Ext/TechnoType/Body.h:859`: the key constructs unset.
#[test]
fn an_absent_key_keeps_the_locomotor_hint() {
    let unset = read("");
    assert_eq!(unset.value, None);
    assert!(unset.exits_through_roof(true, false), "JumpJet alone");
    assert!(unset.exits_through_roof(false, true), "BalloonHover alone");
    assert!(unset.exits_through_roof(true, true));
    assert!(!unset.exits_through_roof(false, false));
    assert!(unset.draws_through_roof(true));
    assert!(
        !unset.draws_through_roof(false),
        "the draw hint is JumpJet alone"
    );
}

/// `Body.cpp:1200`: the type's `yes`/`no` outranks both locomotor hints.
#[test]
fn the_types_value_outranks_the_locomotor_hint() {
    let yes = read("ExitThroughRoof=yes\n");
    assert!(yes.exits_through_roof(false, false));
    assert!(yes.draws_through_roof(false));

    let no = read("ExitThroughRoof=no\n");
    assert!(!no.exits_through_roof(true, true));
    assert!(!no.draws_through_roof(true));
}

/// The two consumers keep **different** fallbacks
/// (`Ext/Building/Hooks.cpp:1053` vs `:1068`).
#[test]
fn the_two_fallbacks_are_not_the_same_predicate() {
    let unset = read("");
    assert_ne!(
        unset.exits_through_roof(false, true),
        unset.draws_through_roof(false),
        "BalloonHover drives the predicate but not the door draw"
    );
}

/// `Nullable<bool>::Read` (`Utilities/TemplateDef.h:1833-1849`): `<default>`,
/// `none` and `<none>` reset the key back to the locomotor hint.
#[test]
fn a_default_or_none_read_clears_the_key() {
    for key in [
        "ExitThroughRoof=<default>",
        "ExitThroughRoof=none",
        "ExitThroughRoof=<none>",
    ] {
        assert_eq!(read(&format!("{key}\n")).value, None, "{key} resets");
    }
    // A blank value never reaches the reader and keeps the unset default.
    assert_eq!(read("ExitThroughRoof=\n").value, None);
}

/// `ObjectType` exposes both native predicates over the same key and the
/// type's locomotor hints (`Ext/Building/Hooks.cpp:1051-1054`, `:1068`).
#[test]
fn object_type_exposes_the_two_roof_exit_predicates() {
    let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=JET\n1=BALLOON\n2=PLAIN\n\
         [JET]\nStrength=100\nJumpJet=yes\n\
         [BALLOON]\nStrength=100\nBalloonHover=yes\n\
         [PLAIN]\nStrength=100\n",
    ))
    .unwrap();
    let jet = rules.object("JET").unwrap();
    assert!(jet.exits_through_roof());
    assert!(jet.draws_through_roof());
    let balloon = rules.object("BALLOON").unwrap();
    assert!(balloon.exits_through_roof());
    assert!(
        !balloon.draws_through_roof(),
        "the door draw defaults to JumpJet alone"
    );
    let plain = rules.object("PLAIN").unwrap();
    assert!(!plain.exits_through_roof());
    assert!(!plain.draws_through_roof());
}
