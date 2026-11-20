//! `BuildLimitGroup.*` reads (`Ext/TechnoType/Body.cpp:1033-1041`) and the two
//! `[General]` fallbacks (`Ext/Rules/Body.cpp:223-224`).
use super::BuildLimitGroupSettings;
use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_globals::PhobosGlobals;

fn read(text: &str) -> BuildLimitGroupSettings {
    BuildLimitGroupSettings::from_section(IniFile::from_str(text).section_or_empty("Q"))
}

/// The constructor defaults, all nine keys, `ValueableVector` token dropping,
/// unresolved names kept for the consumer, and the two `[General]` fallback
/// globals.
#[test]
fn build_limit_group_rules_defaults_paired_reads_and_general_fallbacks() {
    let defaults = BuildLimitGroupSettings::default();
    assert_eq!(read("[Q]\n"), defaults);
    assert!(defaults.types.is_empty() && defaults.nums.is_empty());
    assert_eq!(defaults.factor, 1, "Valueable<int> constructs 1");
    assert_eq!(defaults.content_if_any_match, None, "Nullable is unset");
    assert_eq!(defaults.not_buildable_if_queue_match, None);
    assert!(defaults.extra_limit_types.is_empty());
    assert!(defaults.extra_limit_nums.is_empty());
    assert!(defaults.extra_limit_max_count.is_empty());
    assert_eq!(defaults.extra_limit_max_num, 0);

    let set = read(
        "[Q]\n\
         BuildLimitGroup.Types=A,B,NOT_A_TYPE\n\
         BuildLimitGroup.Nums=2,3,x\n\
         BuildLimitGroup.Factor=4\n\
         BuildLimitGroup.ContentIfAnyMatch=yes\n\
         BuildLimitGroup.NotBuildableIfQueueMatch=no\n\
         BuildLimitGroup.ExtraLimit.Types=EX\n\
         BuildLimitGroup.ExtraLimit.Nums=1\n\
         BuildLimitGroup.ExtraLimit.MaxCount=7\n\
         BuildLimitGroup.ExtraLimit.MaxNum=9\n",
    );
    // Unresolved type names are retained for the consumer, which drops them
    // before pairing (the native reader's `Parser<TechnoTypeClass*>` skip).
    assert_eq!(set.types, ["A", "B", "NOT_A_TYPE"]);
    // `x` fails Parser<int> and is skipped, so `ValueableVector<int>` keeps
    // only the two parsed values (`detail::parse_values`, TemplateDef.h:1728).
    assert_eq!(set.nums, [2, 3]);
    assert_eq!(set.factor, 4);
    assert_eq!(set.content_if_any_match, Some(true));
    assert_eq!(set.not_buildable_if_queue_match, Some(false));
    assert_eq!(set.extra_limit_types, ["EX"]);
    assert_eq!(set.extra_limit_nums, [1]);
    assert_eq!(set.extra_limit_max_count, [7]);
    assert_eq!(set.extra_limit_max_num, 9);

    // `[General]` `Valueable<bool>` globals default false and read from General.
    let globals = PhobosGlobals::default();
    assert!(!globals.build_limit_group_content_if_any_match);
    assert!(!globals.build_limit_group_not_buildable_if_queue_match);
    let globals = PhobosGlobals::from_ini(&IniFile::from_str(
        "[General]\nBuildLimitGroup.ContentIfAnyMatch=yes\n\
         BuildLimitGroup.NotBuildableIfQueueMatch=yes\n",
    ));
    assert!(globals.build_limit_group_content_if_any_match);
    assert!(globals.build_limit_group_not_buildable_if_queue_match);
}
