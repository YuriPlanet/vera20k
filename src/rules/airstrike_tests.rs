use super::AirstrikeSettings;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
use crate::rules::ruleset::RuleSet;
use crate::rules::sound_ini::SoundRegistry;
use std::sync::Arc;

const ROOT: &str = "[InfantryTypes]\n0=BORIS\n[BORIS]\nStrength=200\n\
    AirstrikeTeam=2\nEliteAirstrikeTeam=4\n\
    AirstrikeTeamType=BPLN\nEliteAirstrikeTeamType=OTHER\n\
    AirstrikeRechargeTime=100\nEliteAirstrikeRechargeTime=50\n";

fn layered(root: &str, layer: &str) -> RuleSet {
    let mut stack = RulesLayerStack::new(IniFile::from_str(root));
    stack.push(RulesLayerKind::Scenario, IniFile::from_str(layer));
    RuleSet::from_processed_rules(&stack.process().unwrap()).unwrap()
}

#[test]
fn reached_section_resets_elite_defaults_but_absent_section_preserves_them() {
    let initial = layered(ROOT, "");
    let initial_settings = &initial.object("BORIS").unwrap().airstrike;
    assert_eq!(initial_settings.normal_count, 2);
    assert_eq!(initial_settings.elite_count, 4);
    assert_eq!(initial_settings.normal_type.as_deref(), Some("BPLN"));
    assert_eq!(initial_settings.elite_type.as_deref(), Some("OTHER"));
    assert_eq!(initial_settings.normal_recharge, 100);
    assert_eq!(initial_settings.elite_recharge, 50);
    let missing = layered(ROOT, "[OTHER]\nStrength=500\n");
    assert_eq!(
        &missing.object("BORIS").unwrap().airstrike,
        initial_settings
    );

    let changed = layered(ROOT, "[BORIS]\nAirstrikeTeam=3\n");
    assert_eq!(
        changed.object("BORIS").unwrap().airstrike,
        AirstrikeSettings {
            normal_count: 3,
            elite_count: 3,
            normal_type: Some("BPLN".into()),
            elite_type: Some("BPLN".into()),
            normal_recharge: 100,
            elite_recharge: 100,
        }
    );
}

#[test]
fn aircraft_allocation_order_none_clearing_and_unclamped_integers_survive_projection() {
    let mut stack = RulesLayerStack::new(IniFile::from_str(ROOT));
    let mut processed = stack.process().unwrap();
    let (events, _) = processed.take_constructor_events();
    let allocated: Vec<_> = events
        .iter()
        .map(|event| event.native_stored_id())
        .collect();
    let normal = allocated.iter().position(|id| *id == "BPLN").unwrap();
    let elite = allocated.iter().position(|id| *id == "OTHER").unwrap();
    assert!(normal < elite);
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    assert!(rules.object("BPLN").is_some());
    assert!(rules.object("OTHER").is_some());

    stack.push(
        RulesLayerKind::Scenario,
        IniFile::from_str(
            "[BORIS]\nAirstrikeTeam=-2\nEliteAirstrikeTeam=-4\n\
            AirstrikeTeamType=<none>\nEliteAirstrikeTeamType=NEWPLANE\n\
            AirstrikeRechargeTime=-100\nEliteAirstrikeRechargeTime=-50\n",
        ),
    );
    let rules = RuleSet::from_processed_rules(&stack.process().unwrap()).unwrap();
    let settings = &rules.object("BORIS").unwrap().airstrike;
    assert_eq!(settings.normal_type, None);
    assert_eq!(settings.elite_type.as_deref(), Some("NEWPLANE"));
    assert_eq!((settings.normal_count, settings.elite_count), (-2, -4));
    assert_eq!(
        (settings.normal_recharge, settings.elite_recharge),
        (-100, -50)
    );
    assert!(rules.object("NEWPLANE").is_some());
    for clearing in ["none", "<none>"] {
        let rules = layered(ROOT, &format!("[BORIS]\nAirstrikeTeamType={clearing}\n"));
        let settings = &rules.object("BORIS").unwrap().airstrike;
        assert_eq!(
            (&settings.normal_type, &settings.elite_type),
            (&None, &None)
        );
    }
    let rules = layered(
        ROOT,
        "[BORIS]\nStrength=200\nAirstrikeTeamType=\nEliteAirstrikeTeamType=\n",
    );
    let settings = &rules.object("BORIS").unwrap().airstrike;
    assert_eq!(settings.normal_type.as_deref(), Some("BPLN"));
    assert_eq!(settings.elite_type.as_deref(), Some("BPLN"));
}

#[test]
fn audio_visual_binding_retains_valid_references_across_all_rules_layers() {
    let sounds = Arc::new(SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n0=Abort\n1=Attack\n2=Replacement\n\
         [Abort]\nSounds=a\n[Attack]\nSounds=b\n[Replacement]\nSounds=c\n",
    )));
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        IniFile::from_str(
            "[AudioVisual]\nAirstrikeAbortSound=Abort\n\
            AirstrikeAttackVoice=Attack\nLaserTargetColor=4\n",
        ),
        Some(IniFile::from_str(
            "[AudioVisual]\nAirstrikeAbortSound=\n\
            AirstrikeAttackVoice=Replacement\nLaserTargetColor=7\n",
        )),
        IniFile::empty(),
        sounds,
    )
    .unwrap();
    let (rules, _, _, _) = owner
        .load_scenario(
            NativeScenarioRulesPrefix::NonCampaign(Some(&IniFile::from_str(
                "[AudioVisual]\nAirstrikeAbortSound=Unknown\nAirstrikeAttackVoice=\n",
            ))),
            &IniFile::from_str("[AudioVisual]\nairstrikeabortsound=Attack\nLaserTargetColor=-2\n"),
        )
        .unwrap()
        .into_parts();
    assert_eq!(
        rules.general.airstrike_abort_sound.as_deref(),
        Some("Abort")
    );
    assert_eq!(
        rules.general.airstrike_attack_voice.as_deref(),
        Some("Replacement")
    );
    assert_eq!(rules.general.laser_target_color, -2);
    let empty = RuleSet::from_ini(&IniFile::empty()).unwrap();
    assert_eq!(empty.general.airstrike_abort_sound, None);
    assert_eq!(empty.general.airstrike_attack_voice, None);
    assert_eq!(empty.general.laser_target_color, 0);
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn retail_boris_airstrike_reads_selected_startup_battle_and_map_sources() {
    // Extracted root is mandatory under REQUIRE_RETAIL_INI; archive-backed
    // processing below also checks optional LANGRULE, selected mode and map.
    let Some(_) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = battle.rules;
    assert_eq!(
        rules.object("BORIS").unwrap().airstrike,
        AirstrikeSettings {
            normal_count: 2,
            elite_count: 4,
            normal_type: Some("BPLN".into()),
            elite_type: Some("BPLN".into()),
            normal_recharge: 100,
            elite_recharge: 50,
        }
    );
    assert!(rules.object("BPLN").unwrap().spawned);
    assert_eq!(
        rules.general.airstrike_abort_sound.as_deref(),
        Some("MIGMissionAborted")
    );
    assert_eq!(
        rules.general.airstrike_attack_voice.as_deref(),
        Some("MIGMove")
    );
    assert_eq!(rules.general.laser_target_color, 4);
    assert_eq!(rules.color_add.slots[4].name.as_deref(), Some("HighRed"));
    assert_eq!(rules.color_add.slots[4].rgb, [24, 0, 0]);
}

#[test]
fn equal_current_sources_hash_retained_airstrike_state_and_roundtrip_preserves_it() {
    let mut loaded = Vec::new();
    for count in [1, 2] {
        let prior = RulesLayerStack::new(IniFile::from_str(&format!(
            "[InfantryTypes]\n0=BORIS\n[BORIS]\nStrength=200\nAirstrikeTeam={count}\n"
        )))
        .process()
        .unwrap();
        let (_, trace) = prior.into_ini_and_native_type_construction_trace();
        let (_, _, state) = trace.into_parts();
        let current = RulesLayerStack::new(IniFile::from_str(
            "[InfantryTypes]\n0=BORIS\n[BORIS]\nStrength=200\n",
        ))
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), state)
        .unwrap();
        loaded.push(RuleSet::from_processed_rules(&current).unwrap());
    }
    assert_eq!(loaded[0].source_ini_hash(), loaded[1].source_ini_hash());
    assert_ne!(
        loaded[0].simulation_config_hash(),
        loaded[1].simulation_config_hash()
    );

    let mut stack = RulesLayerStack::new(IniFile::from_str(ROOT));
    stack.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[BORIS]\nAirstrikeTeam=3\n"),
    );
    let processed = stack.process().unwrap();
    let restored = serde_json::from_slice(&serde_json::to_vec(&processed).unwrap()).unwrap();
    let before = RuleSet::from_processed_rules(&processed).unwrap();
    let after = RuleSet::from_processed_rules(&restored).unwrap();
    assert_eq!(
        before.object("BORIS").unwrap().airstrike,
        after.object("BORIS").unwrap().airstrike
    );
    assert_eq!(
        before.simulation_config_hash(),
        after.simulation_config_hash()
    );
}
