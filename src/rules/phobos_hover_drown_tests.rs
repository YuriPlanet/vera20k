//! `HoverDrownable=`: the type's `Nullable` and its `[General]` fallback.

use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_hover_drown::PhobosHoverDrown;

fn read(keys: &str) -> PhobosHoverDrown {
    PhobosHoverDrown::from_section(IniFile::from_str(&format!("[Q]\n{keys}")).section_or_empty("Q"))
}

/// `Ext/UnitType/Body.h:133`: an unset `Nullable`.
#[test]
fn the_key_constructs_unset() {
    assert_eq!(read(""), PhobosHoverDrown::default());
    assert_eq!(PhobosHoverDrown::default().hover_drownable, None);
}

/// `Ext/Techno/Hooks.cpp:1031`: the type's own value wins, and an unset one
/// keeps `[General] HoverDrownable` either way round.
#[test]
fn the_global_applies_only_while_the_type_stays_unset() {
    let unset = PhobosHoverDrown::default();
    assert!(unset.drowns(true), "the global's yes applies");
    assert!(!unset.drowns(false), "the global's no applies");

    let no = read("HoverDrownable=no\n");
    assert_eq!(no.hover_drownable, Some(false));
    assert!(!no.drowns(true), "the type's no wins over the global's yes");

    let yes = read("HoverDrownable=yes\n");
    assert_eq!(yes.hover_drownable, Some(true));
    assert!(yes.drowns(false));
}

/// `Nullable<bool>::Read` (`Utilities/TemplateDef.h:1833-1849`): `<default>`
/// or a blank value resets the override.
#[test]
fn a_default_or_blank_read_clears_the_override() {
    for key in ["HoverDrownable=<default>", "HoverDrownable="] {
        assert_eq!(
            read(&format!("{key}\n")).hover_drownable,
            None,
            "{key} resets"
        );
    }
}
