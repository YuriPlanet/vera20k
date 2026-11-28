//! `AttackMove.*` reads (`Ext/TechnoType/Body.cpp:1086-1087`,
//! `Ext/Rules/Body.cpp:377-378,423`).
use super::PhobosAttackMove;
use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_globals::PhobosGlobals;

#[test]
fn attack_move_type_keys_default_to_unset_with_false_globals() {
    let ini = IniFile::from_str("[MTNK]\n");
    let section = ini.section_or_empty("MTNK");
    let set = PhobosAttackMove::from_section(&section);
    assert_eq!(set, PhobosAttackMove::default());
    assert_eq!((set.aggressive, set.update_target), (None, None));

    let globals = PhobosGlobals::default();
    assert!(!globals.attack_move_aggressive);
    assert!(!globals.attack_move_update_target);
    assert!(!globals.attack_move_ignore_weapon_check);
    // Both unset type keys answer the `[General]` fallback, which is `no`.
    assert!(!set.aggressive(&globals));
    assert!(!set.update_target(&globals));
}

#[test]
fn attack_move_type_keys_override_and_clear_back_to_the_global() {
    let globals = PhobosGlobals {
        attack_move_aggressive: true,
        attack_move_update_target: true,
        ..Default::default()
    };

    let set = PhobosAttackMove::from_section(
        IniFile::from_str("[MTNK]\nAttackMove.Aggressive=yes\nAttackMove.UpdateTarget=no\n")
            .section_or_empty("MTNK"),
    );
    assert_eq!(
        (set.aggressive, set.update_target),
        (Some(true), Some(false))
    );
    assert!(
        set.aggressive(&globals),
        "the type key wins over the global"
    );
    assert!(
        !set.update_target(&globals),
        "an explicit `no` is not the global `yes`"
    );

    // `Phobos Nullable` clears on `<default>`/`none`, so the fallback returns.
    let cleared = PhobosAttackMove::from_section(
        IniFile::from_str(
            "[MTNK]\nAttackMove.Aggressive=<default>\nAttackMove.UpdateTarget=none\n",
        )
        .section_or_empty("MTNK"),
    );
    assert_eq!((cleared.aggressive, cleared.update_target), (None, None));
    assert!(cleared.aggressive(&globals));
    assert!(cleared.update_target(&globals));
}

#[test]
fn attack_move_follow_keys_are_plain_valueables_without_a_global() {
    // `Ext/TechnoType/Body.cpp:1149-1151`: three `Valueable<bool>` keys,
    // constructed `false` (`Body.h:805-807`). Nothing in `[General]` mirrors
    // them, so an absent key is only ever the default.
    let empty =
        PhobosAttackMove::from_section(IniFile::from_str("[MTNK]\n").section_or_empty("MTNK"));
    assert_eq!(
        (
            empty.follow,
            empty.follow_include_air,
            empty.follow_if_mind_control_is_full
        ),
        (false, false, false)
    );

    let set = PhobosAttackMove::from_section(
        IniFile::from_str(
            "[MTNK]\nAttackMove.Follow=yes\nAttackMove.Follow.IncludeAir=yes\n\
             AttackMove.Follow.IfMindControlIsFull=yes\n",
        )
        .section_or_empty("MTNK"),
    );
    assert_eq!(
        (
            set.follow,
            set.follow_include_air,
            set.follow_if_mind_control_is_full
        ),
        (true, true, true)
    );

    // The three keys are independent: `IncludeAir` alone does not enable the
    // follow search and `Follow=no` is not `IfMindControlIsFull=yes`.
    let split = PhobosAttackMove::from_section(
        IniFile::from_str("[MTNK]\nAttackMove.Follow=no\nAttackMove.Follow.IncludeAir=yes\n")
            .section_or_empty("MTNK"),
    );
    assert!(!split.follow);
    assert!(split.follow_include_air);
    assert!(!split.follow_if_mind_control_is_full);
}

#[test]
fn attack_move_general_keys_read_the_three_flags() {
    let globals = PhobosGlobals::from_ini(&IniFile::from_str(
        "[General]\nAttackMove.Aggressive=yes\nAttackMove.UpdateTarget=yes\n\
         AttackMove.IgnoreWeaponCheck=yes\n",
    ));
    assert!(globals.attack_move_aggressive);
    assert!(globals.attack_move_update_target);
    assert!(globals.attack_move_ignore_weapon_check);

    // An absent key keeps the constructor's `false`.
    let empty = PhobosGlobals::from_ini(&IniFile::from_str("[General]\n"));
    assert!(!empty.attack_move_aggressive);
    assert!(!empty.attack_move_update_target);
    assert!(!empty.attack_move_ignore_weapon_check);
}

/// `ApproachTarget.*` (`Ext/TechnoType/Body.cpp:1093-1101`,
/// `Ext/Rules/Body.cpp:642-643`): the deprecated `AttackMove.*` alias is read
/// first and the new key overwrites it, the deprecated pursuit alias is a
/// plain bool, and an unset stop key takes the `[General]` fallback.
#[test]
fn approach_target_keys_prefer_the_new_name_over_the_deprecated_alias() {
    let read = |text: &str| {
        PhobosAttackMove::from_section(IniFile::from_str(text).section_or_empty("MTNK"))
    };
    let deprecated =
        read("[MTNK]\nAttackMove.StopWhenTargetAcquired=yes\nAttackMove.PursuitTarget=yes\n");
    assert_eq!(
        deprecated.approach_target_stop_when_in_range,
        Some(true),
        "the deprecated stop alias fills the field"
    );
    assert!(
        deprecated.approach_target_pursuit_target,
        "the deprecated pursuit alias fills the field"
    );

    let overridden = read(
        "[MTNK]\nAttackMove.StopWhenTargetAcquired=yes\nAttackMove.PursuitTarget=yes\n\
         ApproachTarget.StopWhenInRange=no\nApproachTarget.PursuitTarget=no\n",
    );
    assert_eq!(overridden.approach_target_stop_when_in_range, Some(false));
    assert!(!overridden.approach_target_pursuit_target);

    let globals = PhobosGlobals::from_ini(&IniFile::from_str(
        "[General]\nApproachTarget.StopWhenInRange=yes\n",
    ));
    assert!(globals.approach_target_stop_when_in_range);
    let unset = read("[MTNK]\n");
    assert!(unset.stop_when_in_range(&globals), "the global answers");
    assert!(!unset.approach_target_pursuit_target);
    assert!(!PhobosGlobals::default().approach_target_stop_when_in_range);
}
