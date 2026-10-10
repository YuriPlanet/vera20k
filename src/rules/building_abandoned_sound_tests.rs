//! BuildingAbandonedSound's registered-reference reader, from the original
//! Rules669C23..669C62 instructions. These are reader regressions, not executed
//! native arithmetic comparisons.

use super::RuleSet;
use crate::rules::ini_parser::IniFile;
use crate::rules::process_owner::NativeRulesProcessOwner;
use crate::rules::sound_ini::SoundRegistry;
use std::sync::Arc;

fn sounds() -> SoundRegistry {
    SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n0=GarrisonExit\n1=ReplacementExit\n\
         [GarrisonExit]\nSounds=exit1\n\
         [ReplacementExit]\nSounds=exit2\n",
    ))
}

#[test]
fn building_abandoned_sound_binds_only_exact_key_to_a_registered_name() {
    let sounds = sounds();
    for (text, expected) in [
        ("", None),
        ("[AudioVisual]\nBuildingAbandonedSound=\n", None),
        (
            "[AudioVisual]\nBuildingAbandonedSound=UnregisteredExit\n",
            None,
        ),
        (
            "[AudioVisual]\nBuildingAbandonedSound=GarrisonExit\n",
            Some("GarrisonExit"),
        ),
        (
            "[AudioVisual]\nBuildingAbandonedSound=  gArRiSoNeXiT  \n",
            Some("GarrisonExit"),
        ),
        ("[audiovisual]\nBuildingAbandonedSound=GarrisonExit\n", None),
        ("[AudioVisual]\nbuildingabandonedsound=GarrisonExit\n", None),
        ("[General]\nBuildingAbandonedSound=GarrisonExit\n", None),
    ] {
        let ini = IniFile::from_str(text);
        let mut rules = RuleSet::from_ini(&ini).unwrap();
        assert_eq!(
            rules.general.building_abandoned_sound, None,
            "Rules6658D4 seeds -1 until the fixed sound catalog is bound: {text:?}"
        );
        rules.bind_type_sound_references(&ini, &sounds);
        assert_eq!(
            rules.general.building_abandoned_sound.as_deref(),
            expected,
            "{text:?}"
        );
    }
}

#[test]
fn building_abandoned_sound_retains_current_id_across_production_rules_passes() {
    let sounds = Arc::new(sounds());
    let root = IniFile::from_str("[AudioVisual]\nBuildingAbandonedSound=GarrisonExit\n");
    // Rules669C45/669C52 keep the previous index for an absent, empty or
    // unregistered name. A differently cased INI key never reaches that reader.
    for (lang, mode, map, expected) in [
        ("", "", "", "GarrisonExit"),
        (
            "[AudioVisual]\nBuildingAbandonedSound=\n",
            "[AudioVisual]\nBuildingAbandonedSound=UnregisteredExit\n",
            "[AudioVisual]\nbuildingabandonedsound=ReplacementExit\n",
            "GarrisonExit",
        ),
        (
            "[AudioVisual]\nBuildingAbandonedSound=ReplacementExit\n",
            "[AudioVisual]\nBuildingAbandonedSound=UnregisteredExit\n",
            "[AudioVisual]\nBuildingAbandonedSound=\n",
            "ReplacementExit",
        ),
        (
            "[AudioVisual]\nBuildingAbandonedSound=UnregisteredExit\n",
            "[audiovisual]\nBuildingAbandonedSound=ReplacementExit\n",
            "[AudioVisual]\nBuildingAbandonedSound=rEpLaCeMeNtExIt\n",
            "ReplacementExit",
        ),
    ] {
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            root.clone(),
            Some(IniFile::from_str(lang)),
            IniFile::empty(),
            Arc::clone(&sounds),
        )
        .unwrap();
        let (rules, _, _, _) = owner
            .load_scenario(
                crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(Some(
                    &IniFile::from_str(mode),
                )),
                &IniFile::from_str(map),
            )
            .unwrap()
            .into_parts();
        assert_eq!(
            rules.general.building_abandoned_sound.as_deref(),
            Some(expected),
            "LANGRULE={lang:?}, mode={mode:?}, map={map:?}"
        );
    }
}

#[test]
fn retail_root_building_abandoned_sound_keeps_the_silent_constructor_default() {
    // This checks the installed root rules and fixed sound catalog. Scenario
    // overrides and their retention order are exercised by the layered test.
    let Some((root, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let Some(sound_ini) = crate::rules::retail_ini_fixture::retail_ini("soundmd.ini") else {
        return;
    };
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        root,
        None,
        art,
        Arc::new(SoundRegistry::from_ini(&sound_ini)),
    )
    .unwrap();
    let (rules, _, _, _) = owner
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
            &IniFile::empty(),
        )
        .unwrap()
        .into_parts();
    assert_eq!(rules.general.building_abandoned_sound, None);
}
