//! `FeedbackWeapon`, `VisualScatter` and `AttackNoThreatBuildings` reads
//! (`Ext/WeaponType/Body.cpp:124`, `:133`, `:206`) and the human/AI
//! `[General]` pair that `AttackNoThreatBuildings` falls back to
//! (`Ext/Techno/Hooks.cpp:1852-1858`).

use super::{AreaFireTarget, PhobosWeaponExtras};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;

fn read(text: &str) -> PhobosWeaponExtras {
    PhobosWeaponExtras::from_section(IniFile::from_str(text).section_or_empty("Q"))
}

#[test]
fn weapon_extras_default_to_a_silent_shot() {
    let defaults = PhobosWeaponExtras::default();
    assert_eq!(read("[Q]\n"), defaults);
    assert_eq!(defaults.feedback_weapon, None);
    assert!(!defaults.visual_scatter);
    assert_eq!(defaults.attack_no_threat_buildings, None);
}

#[test]
fn the_three_keys_read_in_their_native_spellings() {
    let set = read(
        "[Q]\n\
         FeedbackWeapon=FBCK\n\
         VisualScatter=yes\n\
         AttackNoThreatBuildings=no\n",
    );
    assert_eq!(set.feedback_weapon.as_deref(), Some("FBCK"));
    assert!(set.visual_scatter);
    assert_eq!(set.attack_no_threat_buildings, Some(false));
}

#[test]
fn attack_no_threat_buildings_falls_back_to_the_human_or_ai_global() {
    // The two `[General]` keys (`Ext/Rules/Body.cpp:517-518`) construct `no`
    // for the human and `yes` for the computer.
    let rules = RuleSet::from_ini(&IniFile::from_str("[General]\n")).expect("rules");
    let unset = read("[Q]\n");
    assert!(!unset.attack_no_threat_buildings(&rules, true));
    assert!(unset.attack_no_threat_buildings(&rules, false));

    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nAutoTarget.NoThreatBuildings=yes\nAutoTargetAI.NoThreatBuildings=no\n",
    ))
    .expect("rules");
    assert!(unset.attack_no_threat_buildings(&rules, true));
    assert!(!unset.attack_no_threat_buildings(&rules, false));

    // A weapon's own key wins over either global.
    let weapon_no = read("[Q]\nAttackNoThreatBuildings=no\n");
    assert!(!weapon_no.attack_no_threat_buildings(&rules, true));
}

/// Phobos `AreaFire.Target=` (`Ext/WeaponType/Body.cpp:123`): the three
/// `AreaFireTarget` names, case-insensitive, and an unknown spelling keeps
/// the current value (`Base` on a fresh read).
#[test]
fn area_fire_target_reads_its_three_names() {
    assert_eq!(
        PhobosWeaponExtras::default().area_fire_target,
        AreaFireTarget::Base
    );
    for (raw, expected) in [
        ("base", AreaFireTarget::Base),
        ("random", AreaFireTarget::Random),
        ("self", AreaFireTarget::Firer),
        ("SELF", AreaFireTarget::Firer),
        ("Random", AreaFireTarget::Random),
    ] {
        let parsed = read(&format!(
            "[Q]
AreaFire.Target={raw}
"
        ));
        assert_eq!(parsed.area_fire_target, expected, "{raw}");
    }
    assert_eq!(
        read(
            "[Q]
AreaFire.Target=nearest
"
        )
        .area_fire_target,
        AreaFireTarget::Base,
        "an unknown name keeps the previous value"
    );
}

