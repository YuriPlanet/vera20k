//! Reader tests for the four `NoRearm.*` / `NoReload.*` type keys.

use super::PhobosNoRearm;
use crate::rules::ini_parser::IniFile;

#[test]
fn unset_keys_read_none_and_set_keys_read_their_value() {
    let ini = IniFile::from_str(
        "[A]\nNoRearm.UnderEMP=yes\nNoRearm.Temporal=no\nNoReload.UnderEMP=1\n\n\
         [B]\nStrength=100\n",
    );
    let a = PhobosNoRearm::from_section(ini.section("A").unwrap());
    assert_eq!(a.rearm_under_emp, Some(true));
    assert_eq!(a.rearm_temporal, Some(false));
    assert_eq!(a.reload_under_emp, Some(true));
    assert_eq!(a.reload_temporal, None);
    assert_eq!(
        PhobosNoRearm::from_section(ini.section("B").unwrap()),
        PhobosNoRearm::default(),
        "a section without the keys stays unset"
    );
}

#[test]
fn effective_values_fall_back_to_the_global() {
    let keys = PhobosNoRearm {
        rearm_under_emp: Some(false),
        ..PhobosNoRearm::default()
    };
    assert!(
        !keys.rearm_under_emp(true),
        "the type key overrides the global"
    );
    assert!(
        keys.rearm_temporal(true) && keys.reload_under_emp(true) && keys.reload_temporal(true),
        "unset keys take the global"
    );
}
