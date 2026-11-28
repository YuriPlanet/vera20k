//! `XDrawOffset.*` / `YDrawOffset.*` reads (`Ext/AnimType/Body.cpp:82-90`).
use super::PhobosAnimDrawOffset;
use crate::rules::ini_parser::IniFile;

#[test]
fn draw_offset_defaults_match_the_anim_type_constructor() {
    let set =
        PhobosAnimDrawOffset::from_section(IniFile::from_str("[ANIM]\n").section_or_empty("ANIM"));
    assert_eq!(set, PhobosAnimDrawOffset::default());
    assert!(!set.applies_bracket());
    assert_eq!(set.x_adjust(true), 0, "unset building override falls back");
    assert_eq!(set.y_adjust(true), 0, "unset building override falls back");
}

#[test]
fn draw_offset_reads_both_bracket_arms_and_the_building_overrides() {
    let set = PhobosAnimDrawOffset::from_section(
        IniFile::from_str(
            "[ANIM]\nXDrawOffset=30\nXDrawOffset.ApplyBracketWidth=yes\n\
             XDrawOffset.InvertBracketShift=yes\nXDrawOffset.BracketAdjust=5\n\
             XDrawOffset.BracketAdjust.Buildings=-7\n\
             YDrawOffset.ApplyBracketHeight=yes\nYDrawOffset.InvertBracketShift=no\n\
             YDrawOffset.BracketAdjust=9\nYDrawOffset.BracketAdjust.Buildings=2\n",
        )
        .section_or_empty("ANIM"),
    );
    assert_eq!(set.x_draw_offset, 30);
    assert!(set.x_apply_bracket_width && set.x_invert_bracket_shift);
    assert!(set.y_apply_bracket_height && !set.y_invert_bracket_shift);
    assert_eq!(set.x_bracket_adjust, 5);
    assert_eq!(set.x_bracket_adjust_buildings, Some(-7));
    assert_eq!(set.y_bracket_adjust, 9);
    assert_eq!(set.y_bracket_adjust_buildings, Some(2));
    assert_eq!((set.x_adjust(true), set.x_adjust(false)), (-7, 5));
    assert_eq!((set.y_adjust(true), set.y_adjust(false)), (2, 9));
}

#[test]
fn draw_offset_building_override_none_clears_to_the_plain_adjust() {
    let set = PhobosAnimDrawOffset::from_section(
        IniFile::from_str(
            "[ANIM]\nXDrawOffset.BracketAdjust=4\n\
             XDrawOffset.BracketAdjust.Buildings=none\n",
        )
        .section_or_empty("ANIM"),
    );
    assert_eq!(set.x_bracket_adjust_buildings, None);
    assert_eq!(set.x_adjust(true), 4);
}
