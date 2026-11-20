//! `KeepTargetOnMove.*` reads (`Ext/TechnoType/Body.cpp:1103-1106`).
use super::PhobosKeepTargetOnMove;
use crate::rules::ini_parser::IniFile;

fn read(text: &str) -> PhobosKeepTargetOnMove {
    PhobosKeepTargetOnMove::from_section(IniFile::from_str(text).section_or_empty("Q"))
}

#[test]
fn keep_target_on_move_defaults_and_all_four_keys() {
    let defaults = PhobosKeepTargetOnMove::default();
    assert_eq!(read("[Q]\n"), defaults);
    assert!(!defaults.enabled);
    assert_eq!(defaults.weapon, -1);
    assert!(defaults.no_more_pursuit, "NoMorePursuit constructs yes");
    assert_eq!(defaults.extra_distance, 0);

    let set = read(
        "[Q]\nKeepTargetOnMove=yes\nKeepTargetOnMove.Weapon=1\n\
         KeepTargetOnMove.NoMorePursuit=no\nKeepTargetOnMove.ExtraDistance=1.5\n",
    );
    assert!(set.enabled);
    assert_eq!(set.weapon, 1);
    assert!(!set.no_more_pursuit);
    // `Valueable<Leptons>`: cells × 256, F2I.
    assert_eq!(set.extra_distance, 384);
}
