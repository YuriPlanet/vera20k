//! Phobos `AmphibiousEnter=` / `AmphibiousUnload=` resolution
//! (`Ext/TechnoType/Body.cpp:1043-1044`, `Ext/Rules/Body.cpp:195-196`): a
//! transport type's `Nullable<bool>` wins; an unset type key answers the
//! `[General]` switch. `ObjectType::amphibious_enter` / `_unload` are the one
//! place that precedence is defined, so both keys are checked here together.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;

fn rules(general: &str, type_keys: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\n{general}[VehicleTypes]\n0=LCRF\n[LCRF]\nPassengers=5\n{type_keys}"
    )))
    .expect("transport rules parse")
}

fn resolved(general: &str, type_keys: &str) -> (bool, bool) {
    let rules = rules(general, type_keys);
    let transport = rules.object("LCRF").expect("LCRF");
    (
        transport.amphibious_enter(&rules),
        transport.amphibious_unload(&rules),
    )
}

/// Type `yes` over global `no`, type `no` over global `yes`, and an unset type
/// key answers the global in both directions. Both keys default to `no`.
#[test]
fn the_override_falls_back_to_the_global() {
    assert_eq!(resolved("", ""), (false, false));
    assert_eq!(
        resolved("AmphibiousEnter=yes\nAmphibiousUnload=yes\n", ""),
        (true, true)
    );
    assert_eq!(
        resolved(
            "AmphibiousEnter=yes\nAmphibiousUnload=yes\n",
            "AmphibiousEnter=no\nAmphibiousUnload=no\n",
        ),
        (false, false)
    );
    assert_eq!(
        resolved("", "AmphibiousEnter=yes\nAmphibiousUnload=yes\n"),
        (true, true)
    );
}
