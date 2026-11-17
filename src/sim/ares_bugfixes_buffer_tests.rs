//! Ares 3.0 list-buffer fixes (`ares-src/src/Misc/Bugfixes.Buffer.cpp`)
//! through the production readers. Each test names its hooks.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;

/// 130 empty comma tokens: strtok skips them, but they fill the original
/// 128-byte buffer so that a name after them is cut off.
fn past_128() -> String {
    ",".repeat(130)
}

fn rules(source: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(source)).expect("buffer rules")
}

/// Buf_General (1005BB00, hook 0x66D55E) and Buf_AI (1005B830, hook
/// 0x672B0E): ParseList reads the lists through the 0x800 buffer and
/// Find-only, so a name past 128 bytes counts and an unknown one is neither
/// listed nor allocated; ParseList<int> (1005B030) drops a token that does
/// not convert where the original atoi stored 0.
#[test]
fn ares_rules_lists_read_long_values_and_skip_unknown_names() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules(&format!(
        "[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=MCV\n[BuildingTypes]\n0=YARD\n\
         [E1]\nStrength=100\n[MCV]\nStrength=100\n[YARD]\nStrength=100\n\
         [General]\nAmerParaDropInf={},E1\nAmerParaDropNum=3,x,2\nBaseUnit=NOSUCH,MCV\n\
         [AI]\nBuildConst=NOYARD,YARD\nAIForcePredictionFudge=5,junk,80\n",
        past_128()
    ));
    assert_eq!(rules.general.amer_paradrop.infantry, ["E1"]);
    assert_eq!(rules.general.amer_paradrop.counts, [3, 2]);
    assert_eq!(rules.general.base_unit_types, ["MCV"]);
    assert_eq!(rules.build_const_types, ["YARD"]);
    assert_eq!(rules.general.ai_force_prediction_fudge, [5, 80]);
    assert!(rules.object("NOSUCH").is_none() && rules.object("NOYARD").is_none());
}

/// TechnoTypeClass_LoadFromINI_ListLength (1005BF90, hook 0x7125DF),
/// WarheadTypeClass_LoadFromINI_ListLength (1005C080, hook 0x75D660) and
/// WeaponTypeClass_LoadFromINI_ListLength (1005C0F0, hook 0x772462): the
/// type lists are Find-only and the debris maximums skip bad tokens.
#[test]
fn ares_type_lists_find_existing_types_only() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules(
        "[VehicleTypes]\n0=TANK\n[BuildingTypes]\n0=DEPOT\n[Animations]\n0=EXPA\n\
         [Warheads]\n0=HE\n\
         [TANK]\nStrength=100\nPrimary=Gun\nDock=NODEPOT,DEPOT\nDebrisMaximums=2,x,1\n\
         [DEPOT]\nStrength=100\n\
         [Gun]\nDamage=10\nRange=5\nWarhead=HE\nAnim=NOANIM,EXPA\n\
         [HE]\nAnimList=NOANIM,EXPA\nDebrisMaximums=x,4\n",
    );
    let tank = rules.object("TANK").unwrap();
    assert_eq!(tank.dock, ["DEPOT"]);
    assert_eq!(tank.debris_maximums, [2, 1]);
    assert_eq!(rules.weapon("Gun").unwrap().anim, ["EXPA"]);
    let warhead = rules.warhead("HE").unwrap();
    assert_eq!(warhead.anim_list, ["EXPA"]);
    assert_eq!(warhead.debris_maximums, [4]);
    assert!(rules.object("NODEPOT").is_none());
}

/// INIClass_ReadHouseTypesList (1005BED0, hook 0x4750EC; strtok 1005BEF0,
/// hook 0x475107): a house named past 128 bytes still enters the mask.
#[test]
fn ares_house_lists_read_past_128_bytes() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules(&format!(
        "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=TANK\n\
         [TANK]\nStrength=100\nRequiredHouses={},Russians\n",
        past_128()
    ));
    assert_eq!(
        rules.object("TANK").unwrap().required_houses_mask(&rules),
        0b10
    );
}
