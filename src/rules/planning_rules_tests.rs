//! Rules constructor/read evidence:665725,6658F2/F8,665904,671DB2..DCC,
//! 6696C0..669740 and669B25..B52. These are production-reader regression
//! checks; they do not establish gameplay planning parity.

use std::sync::Arc;

use super::NativeRulesProcessOwner;
use crate::rules::ini_parser::IniFile;
use crate::rules::process_owner::NativeScenarioRulesPrefix;
use crate::rules::ruleset::{GeneralRules, RuleSet};
use crate::rules::sound_ini::SoundRegistry;

#[test]
fn planning_constructor_and_signed_integer_reader_keep_native_defaults() {
    let defaults = GeneralRules::default();
    assert_eq!(defaults.max_waypoint_path_length, 15);
    assert!(defaults.start_planning_mode_sound.is_none());
    assert!(defaults.end_planning_mode_sound.is_none());
    assert!(defaults.add_planning_mode_command_sound.is_none());
    for (text, expected) in [
        ("", 15),
        ("[General]\nMaxWaypointPathLength=\n", 15),
        ("[general]\nMaxWaypointPathLength=7\n", 15),
        ("[General]\nmaxwaypointpathlength=7\n", 15),
        ("[AudioVisual]\nMaxWaypointPathLength=7\n", 15),
        ("[General]\nMaxWaypointPathLength=-3\n", -3),
        ("[General]\nMaxWaypointPathLength=0\n", 0),
        ("[General]\nMaxWaypointPathLength=$20\n", 32),
        ("[General]\nMaxWaypointPathLength=23tail\n", 23),
        ("[General]\nMaxWaypointPathLength=junk\n", 0),
    ] {
        assert_eq!(
            RuleSet::from_ini(&IniFile::from_str(text))
                .unwrap()
                .general
                .max_waypoint_path_length,
            expected,
            "{text:?}"
        );
    }
}

#[test]
fn planning_sounds_and_limit_follow_root_lang_mode_map_reader_history() {
    let ini = IniFile::from_str;
    let sounds = Arc::new(SoundRegistry::from_ini(&ini(
        "[SoundList]\n0=Start\n1=End\n2=Add\n3=Replacement\n4=Wrong\n[Unregistered]\nSounds=sample\n",
    )));
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        ini("[General]\nMaxWaypointPathLength=17\nStartPlanningModeSound=Wrong\n\
             [AudioVisual]\nStartPlanningModeSound=Start\nEndPlanningModeSound=End\nAddPlanningModeCommandSound=Add\n"),
        Some(ini("[General]\nMaxWaypointPathLength=29\n[AudioVisual]\nStartPlanningModeSound=unknown\nEndPlanningModeSound=\n")),
        ini(""),
        Arc::clone(&sounds),
     Default::default())
    .unwrap();
    let mode = ini(
        "[General]\nMaxWaypointPathLength=$20\n[AudioVisual]\nEndPlanningModeSound= rEpLaCeMeNt \n",
    );
    let map = ini(
        "[General]\nMaxWaypointPathLength=-2\n[AudioVisual]\nStartPlanningModeSound=\nEndPlanningModeSound=Unregistered\nAddPlanningModeCommandSound=unknown\naddplanningmodecommandsound=Wrong\n",
    );
    let (rules, _, _, _) = owner
        .load_scenario(NativeScenarioRulesPrefix::NonCampaign(Some(&mode)), &map)
        .unwrap()
        .into_parts();
    assert!(Arc::ptr_eq(owner.fixed_sounds(), &sounds));
    assert_eq!(rules.general.max_waypoint_path_length, -2);
    assert_eq!(
        rules.general.start_planning_mode_sound.as_deref(),
        Some("Start")
    );
    assert_eq!(
        rules.general.end_planning_mode_sound.as_deref(),
        Some("Replacement")
    );
    assert_eq!(
        rules.general.add_planning_mode_command_sound.as_deref(),
        Some("Add")
    );
}

#[test]
fn planning_sound_missing_catalog_or_wrong_section_keeps_constructor_silence() {
    let ini = IniFile::from_str;
    for catalog in [ini(""), ini("[SoundList]\n0=Start\n")] {
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            ini(
                "[General]\nStartPlanningModeSound=Start\nEndPlanningModeSound=Start\n\
                 [AudioVisual]\nAddPlanningModeCommandSound=Unregistered\n",
            ),
            None,
            ini(""),
            Arc::new(SoundRegistry::from_ini(&catalog)),
            Default::default(),
        )
        .unwrap();
        let (rules, _, _, _) = owner
            .load_scenario(NativeScenarioRulesPrefix::NonCampaign(None), &ini(""))
            .unwrap()
            .into_parts();
        assert!(rules.general.start_planning_mode_sound.is_none());
        assert!(rules.general.end_planning_mode_sound.is_none());
        assert!(rules.general.add_planning_mode_command_sound.is_none());
    }
}

#[test]
fn retail_planning_rules_use_production_reader_and_fixed_soundmd_catalog() {
    use crate::rules::retail_ini_fixture::retail_ini;
    let Some(root) = retail_ini("rulesmd.ini") else {
        return;
    };
    let Some(soundmd) = retail_ini("soundmd.ini") else {
        return;
    };
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        root,
        None,
        IniFile::empty(),
        Arc::new(SoundRegistry::from_ini(&soundmd)),
        Default::default(),
    )
    .unwrap();
    let (rules, _, _, _) = owner
        .load_scenario(
            NativeScenarioRulesPrefix::NonCampaign(None),
            &IniFile::empty(),
        )
        .unwrap()
        .into_parts();
    assert_eq!(rules.general.max_waypoint_path_length, 15);
    assert_eq!(
        rules.general.start_planning_mode_sound.as_deref(),
        Some("PlanningModeStart")
    );
    assert_eq!(
        rules.general.end_planning_mode_sound.as_deref(),
        Some("PlanningModeEnd")
    );
    assert_eq!(
        rules.general.add_planning_mode_command_sound.as_deref(),
        Some("PlanningModeAdd")
    );
}

#[test]
fn retail_battle_planning_rules_cover_selected_root_mode_and_map_passes() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    assert_eq!(retail.rules.general.max_waypoint_path_length, 15);
    assert_eq!(
        retail.rules.general.start_planning_mode_sound.as_deref(),
        Some("PlanningModeStart")
    );
    assert_eq!(
        retail.rules.general.end_planning_mode_sound.as_deref(),
        Some("PlanningModeEnd")
    );
    assert_eq!(
        retail
            .rules
            .general
            .add_planning_mode_command_sound
            .as_deref(),
        Some("PlanningModeAdd")
    );
}
