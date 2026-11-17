//! BuildingTypeClass::GetPipMax (0x0045ECE0) through the production readers.
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::RulesLayerStack;
use crate::rules::ruleset::RuleSet;

/// Retail refineries cap their Tiberium pips at foundation width x 60 / 8,
/// below their `Storage=200`: GAREFN and NAREFN (4x3) at 30, YAREFN (2x2) at
/// 15. Retail `[General] WeedCapacity=` is 56.
#[test]
fn retail_refineries_cap_tiberium_pips_by_foundation_width() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    for (name, pips) in [("GAREFN", 30), ("NAREFN", 30), ("YAREFN", 15)] {
        assert_eq!(rules.object(name).unwrap().pip_max(rules), pips, "{name}");
    }
    assert_eq!(rules.general.weed_capacity, 56);
}

/// A Weeder takes `[General] WeedCapacity=` instead of `Storage=`, a negative
/// capacity answers 0 (0x0045ED2E, 0x0045ED42), and Power answers the cap.
#[test]
fn weeder_negative_storage_and_power_building_pip_max() {
    let art = IniFile::from_str("[SMALL]\nFoundation=2x2\n[WIDE]\nFoundation=3x2\n");
    let ini = IniFile::from_str(
        "[General]\nWeedCapacity=7\n\
         [Countries]\n0=Americans\n[Americans]\nSide=GDI\n\
         [BuildingTypes]\n0=WEED\n1=EMPTY\n2=PLANT\n\
         [WEED]\nImage=SMALL\nStrength=100\nWeeder=yes\nPipScale=Tiberium\nStorage=200\n\
         [EMPTY]\nImage=SMALL\nStrength=100\nPipScale=Tiberium\nStorage=-1\n\
         [PLANT]\nImage=WIDE\nStrength=100\nPipScale=Power\n",
    );
    let processed = RulesLayerStack::new(ini)
        .process_with_fixed_art(&art)
        .unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    for (name, pips) in [("WEED", 7), ("EMPTY", 0), ("PLANT", 22)] {
        assert_eq!(rules.object(name).unwrap().pip_max(&rules), pips, "{name}");
    }
}
