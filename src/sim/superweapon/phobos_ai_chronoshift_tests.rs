//! Phobos `HouseExt::GetAIChronoshiftSupers` (`Ext/House/Body.cpp:333-368`):
//! the `[General] AIChronoSphereSW=` / `AIChronoWarpSW=` superweapon names and
//! the sphere's `SW.PostDependent=` name ahead of the native `Type=` scan,
//! which still runs — and overwrites — whenever the pair is incomplete. The
//! keys hold type IDs, not numbers (`ValueableIdx::Read`,
//! `Utilities/TemplateDef.h:1814-1827`).
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::superweapon::ai_chronoshift_super_indices;

fn read_rules(text: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(text)).expect("rules")
}

/// Four entries, the last two being the ChronoSphere and ChronoWarp the scan
/// finds, the first two ordinary supers the keys can name.
const LIST: &str = "[SuperWeaponTypes]\n0=PARA\n1=SPHERE_A\n2=SPHERE_B\n3=WARP_A\n\
                   [PARA]\nType=ParaDrop\n\
                   [SPHERE_A]\nType=ChronoSphere\n\
                   [SPHERE_B]\nType=ChronoSphere\n\
                   [WARP_A]\nType=ChronoWarp\n";

/// Both names present: they are the pair, and the scan never runs
/// (`Ext/House/Body.cpp:339-356`).
#[test]
fn the_authored_supers_precede_the_type_scan() {
    let rules = read_rules(&format!(
        "[General]\nAIChronoSphereSW=PARA\nAIChronoWarpSW=SPHERE_A\n{LIST}"
    ));
    assert_eq!(
        ai_chronoshift_super_indices(&rules),
        (Some(0), Some(1)),
        "`Supers[0]`/`Supers[1]` are the sphere and warp whatever their `Type=`"
    );
}

/// Only the sphere name: `Ext/House/Body.cpp:343-349` lets that type's
/// `SW.PostDependent=` (`Ext/SWType/Body.cpp:146`) answer for the warp.
#[test]
fn the_sphere_post_dependent_names_the_warp_when_only_the_sphere_is_named() {
    let rules = read_rules(
        "[General]\nAIChronoSphereSW=SPHERE_A\n\
         [SuperWeaponTypes]\n0=PARA\n1=SPHERE_A\n2=WARP_A\n3=SPHERE_B\n\
         [PARA]\nType=ParaDrop\n\
         [SPHERE_A]\nType=ChronoSphere\nSW.PostDependent=WARP_A\n\
         [WARP_A]\nType=ChronoWarp\n\
         [SPHERE_B]\nType=ChronoSphere\n",
    );
    assert_eq!(ai_chronoshift_super_indices(&rules), (Some(1), Some(2)));

    // A sphere type without the key leaves the warp unfound, so the scan runs
    // and its last ChronoSphere overwrites the named one.
    let no_post = read_rules(&format!("[General]\nAIChronoSphereSW=SPHERE_A\n{LIST}"));
    assert_eq!(
        ai_chronoshift_super_indices(&no_post),
        (Some(2), Some(3)),
        "the scan re-assigns both pointers, not only the missing one"
    );
}

/// With neither key (the retail state) the native loop alone decides, taking
/// the last entry of each type.
#[test]
fn unset_names_leave_the_type_scan_to_choose() {
    let rules = read_rules(&format!("[General]\nFixture=1\n{LIST}"));
    assert_eq!(ai_chronoshift_super_indices(&rules), (Some(2), Some(3)));

    let no_warp = read_rules(
        "[General]\nAIChronoSphereSW=SPHERE_A\n\
         [SuperWeaponTypes]\n0=PARA\n1=SPHERE_A\n\
         [PARA]\nType=ParaDrop\n\
         [SPHERE_A]\nType=ChronoSphere\n",
    );
    assert_eq!(
        ai_chronoshift_super_indices(&no_warp).1,
        None,
        "no ChronoWarp anywhere means the action finishes unfired"
    );
}

/// `ValueableIdx::Read` stores the unset index for `none` and keeps what it
/// had for a name `FindIndex` does not know, so both are unfound here and the
/// scan decides (`Utilities/TemplateDef.h:1820-1825`).
#[test]
fn a_name_matching_no_type_counts_as_unfound() {
    let unknown = read_rules(&format!(
        "[General]\nAIChronoSphereSW=NOSUCH\nAIChronoWarpSW=7\n{LIST}"
    ));
    assert_eq!(
        ai_chronoshift_super_indices(&unknown),
        (Some(2), Some(3)),
        "both fall back to the last ChronoSphere and ChronoWarp"
    );

    let cleared = read_rules(&format!(
        "[General]\nAIChronoSphereSW=SPHERE_A\nAIChronoWarpSW=none\n{LIST}"
    ));
    assert_eq!(
        ai_chronoshift_super_indices(&cleared),
        (Some(2), Some(3)),
        "`none` is the unset index, so only the scan's pair stands"
    );
}
